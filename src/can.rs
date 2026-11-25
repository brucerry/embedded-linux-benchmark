use crate::common::Deadline;
use crate::{linux_poll, linux_socket};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::raw::{c_char, c_uint};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::time::Instant;

pub(crate) const MAX_FRAMES: u64 = 4_096;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;
const AF_CAN: i32 = 29;
const CAN_RAW: i32 = 1;
const SOL_CAN_RAW: i32 = 101;
const RAW_FILTER: i32 = 1;
const RAW_LOOPBACK: i32 = 3;
const RAW_FD_FRAMES: i32 = 5;
const FILTER_MASK: u32 = 0xc000_07ff; // EFF and RTR flags, and all standard ID bits.
const CAN_MTU: usize = 16;
const CANFD_MTU: usize = 72;
const HEADER: usize = 8;

pub(crate) struct Config<'a> {
    pub(crate) interface: &'a str,
    pub(crate) tx: u32,
    pub(crate) rx: u32,
    pub(crate) session: u32,
    pub(crate) frames: u32,
    pub(crate) fd: bool,
}

pub(crate) fn interface(name: &str) -> Result<&str, String> {
    if name.is_empty()
        || name.len() > 15
        || matches!(name, "." | "..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
    {
        Err("'--interface' must be a Linux interface name of 1–15 characters".into())
    } else {
        Ok(name)
    }
}

pub(crate) fn identifier(text: &str) -> Result<u32, String> {
    let result = match text.strip_prefix("0x") {
        Some(hex) => u32::from_str_radix(hex, 16),
        None => text.parse(),
    };
    result
        .ok()
        .filter(|id| *id <= 0x7ff)
        .ok_or_else(|| "CAN identifier must be a standard 11-bit ID (0 to 0x7ff)".into())
}

pub(crate) fn session(text: &str) -> Result<u32, String> {
    if text.len() != 8 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("CAN '--session' must contain exactly 8 hexadecimal characters".into());
    }
    u32::from_str_radix(text, 16).map_err(|error| error.to_string())
}

#[repr(C)]
struct Address {
    family: u16,
    index: i32,
    address: [u64; 2],
}

const _: [(); 24] = [(); std::mem::size_of::<Address>()];

extern "C" {
    fn if_nametoindex(name: *const c_char) -> c_uint;
}

impl Config<'_> {
    fn mtu(&self) -> usize {
        if self.fd {
            CANFD_MTU
        } else {
            CAN_MTU
        }
    }

    fn pattern(&self, identifier: u32, sequence: u32) -> [u8; CANFD_MTU] {
        let mut frame = [0; CANFD_MTU];
        frame[..4].copy_from_slice(&identifier.to_ne_bytes());
        frame[4] = (self.mtu() - HEADER) as u8;
        frame[HEADER..12].copy_from_slice(&self.session.to_be_bytes());
        frame[12..16].copy_from_slice(&sequence.to_be_bytes());
        if self.fd {
            for (offset, byte) in frame[16..].iter_mut().enumerate() {
                *byte = (self.session ^ sequence ^ (offset as u32).wrapping_mul(79)) as u8;
            }
        }
        frame
    }

    fn verify(&self, frame: &[u8], sequence: u32) -> Result<(), String> {
        let expected = self.pattern(self.rx, sequence);
        // The kernel can set CANFD_FDF and other metadata in bytes 5..8.
        if frame.len() != self.mtu()
            || frame[..5] != expected[..5]
            || frame[HEADER..] != expected[HEADER..self.mtu()]
        {
            return Err(format!(
                "CAN fixture frame {sequence} has an unexpected ID, size, session, or payload"
            ));
        }
        Ok(())
    }
}

fn open(config: &Config<'_>) -> Result<File, String> {
    let path = Path::new("/sys/class/net").join(config.interface);
    let read = |name: &str| {
        fs::read_to_string(path.join(name))
            .map_err(|error| format!("cannot inspect CAN interface {}: {error}", config.interface))
    };
    if read("type")?.trim() != "280" {
        return Err(format!("{} is not a SocketCAN interface", config.interface));
    }
    let mtu = read("mtu")?;
    if config.fd && mtu.trim() != "72" {
        return Err("CAN FD requires an interface already configured with a CAN FD MTU".into());
    }
    if !matches!(mtu.trim(), "16" | "72") {
        return Err("CAN benchmark requires a Classic CAN or CAN FD interface".into());
    }
    let name = CString::new(config.interface).map_err(|error| error.to_string())?;
    let index = unsafe { if_nametoindex(name.as_ptr()) };
    if index == 0 || index > i32::MAX as u32 {
        return Err(format!(
            "cannot find CAN interface {}: {}",
            config.interface,
            io::Error::last_os_error()
        ));
    }
    let fd = linux_socket::open(AF_CAN, linux_socket::RAW, CAN_RAW)
        .map_err(|error| format!("SocketCAN raw sockets unavailable: {error}"))?;
    let filter = [config.rx, FILTER_MASK];
    // These options apply only to this socket. Loopback is disabled so a kernel-local send
    // cannot verify a physical link. A separate peer replies on a different identifier.
    unsafe {
        linux_socket::option(&fd, SOL_CAN_RAW, RAW_FILTER, &filter)
            .map_err(|error| format!("cannot set the CAN receive filter: {error}"))?;
        linux_socket::option(&fd, SOL_CAN_RAW, RAW_LOOPBACK, &0i32)
            .map_err(|error| format!("cannot disable local CAN echo: {error}"))?;
        if config.fd {
            linux_socket::option(&fd, SOL_CAN_RAW, RAW_FD_FRAMES, &1i32)
                .map_err(|error| format!("CAN FD unavailable on this socket: {error}"))?;
        }
        linux_socket::bind_to(
            &fd,
            &Address {
                family: AF_CAN as u16,
                index: index as i32,
                address: [0; 2],
            },
        )
        .map_err(|error| format!("cannot bind CAN socket: {error}"))?;
    }
    Ok(File::from(fd))
}

fn receive(mut file: &File, frame: &mut [u8], deadline: Deadline) -> Result<usize, String> {
    loop {
        deadline.check()?;
        match file.read(frame) {
            Ok(count) => return Ok(count),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                linux_poll::wait(file.as_raw_fd(), linux_poll::READ, deadline)?
            }
            Err(error) => return Err(format!("CAN receive failed: {error}")),
        }
    }
}

fn send(mut file: &File, frame: &[u8], deadline: Deadline) -> Result<(), String> {
    loop {
        deadline.check()?;
        match file.write(frame) {
            Ok(count) if count == frame.len() => return Ok(()),
            Ok(count) => return Err(format!("CAN sent {count} of {} frame bytes", frame.len())),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                linux_poll::wait(file.as_raw_fd(), linux_poll::WRITE, deadline)?
            }
            Err(error) => return Err(format!("CAN send failed: {error}")),
        }
    }
}

fn echo(
    config: &Config<'_>,
    sender: bool,
    deadline: Deadline,
    mut tx: impl FnMut(&[u8]) -> Result<(), String>,
    mut rx: impl FnMut(&mut [u8]) -> Result<usize, String>,
) -> Result<(), String> {
    let mut received = [0; CANFD_MTU];
    for seq in 0..config.frames {
        deadline.check()?;
        let outgoing = config.pattern(config.tx, seq);
        if sender {
            tx(&outgoing[..config.mtu()])?;
        }
        let count = rx(&mut received)?;
        config.verify(&received[..count], seq)?;
        if !sender {
            tx(&outgoing[..config.mtu()])?;
        }
    }
    deadline.check()?;
    Ok(())
}

pub(crate) fn run(config: &Config<'_>, sender: bool, deadline: Deadline) -> Result<(), String> {
    deadline.check()?;
    let file = open(config)?;
    let start = Instant::now();
    echo(
        config,
        sender,
        deadline,
        |buf| send(&file, buf, deadline),
        |buf| receive(&file, buf, deadline),
    )?;
    let elapsed = start.elapsed().as_secs_f64();
    let payload_bytes = u64::from(config.frames) * (config.mtu() - HEADER) as u64;
    report_line!(
        "{} (isolated SocketCAN peer fixture)",
        if config.fd { "CAN FD" } else { "Classic CAN" }
    );
    report_line!("Interface: {}", config.interface);
    report_line!("Frames verified: {}", config.frames);
    report_line!("Payload bytes verified: {payload_bytes}");
    report_line!("Elapsed: {elapsed:.6} s");
    if sender {
        crate::report::metric(
            "CAN round trip rate",
            f64::from(config.frames) / elapsed,
            "round trips/s",
        );
        crate::report::metric(
            "Mean CAN round-trip cycle",
            elapsed * 1000.0 / f64::from(config.frames),
            "ms",
        );
        report_line!(
            "Verified round trips/s: {:.2}",
            f64::from(config.frames) / elapsed
        );
        report_line!(
            "Mean round-trip cycle: {:.3} ms",
            elapsed * 1000.0 / f64::from(config.frames)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn isolated_mock_echo_checks_classic_and_fd_wire_payloads() {
        for fd in [false, true] {
            for sender in [false, true] {
                let config = Config {
                    interface: "fixture",
                    tx: 0x123,
                    rx: 0x124,
                    session: 0x12345678,
                    frames: 4,
                    fd,
                };
                let mut sent = 0;
                let mut received = 0;
                let deadline = Deadline::after(Duration::from_secs(2));
                echo(
                    &config,
                    sender,
                    deadline,
                    |frame| {
                        assert_eq!(frame, &config.pattern(config.tx, sent)[..config.mtu()]);
                        sent += 1;
                        Ok(())
                    },
                    |frame| {
                        frame.copy_from_slice(&config.pattern(config.rx, received));
                        frame[5] = 4;
                        received += 1;
                        Ok(config.mtu())
                    },
                )
                .unwrap();
                assert_eq!((sent, received), (4, 4));
                let mut bad = config.pattern(config.rx, 0);
                bad[8] ^= 1;
                assert!(config.verify(&bad[..config.mtu()], 0).is_err());
                assert!(config.verify(&bad[..3], 0).is_err());
            }
        }
    }
}

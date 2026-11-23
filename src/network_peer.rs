use crate::common::{Deadline, MIB};
use crate::linux_poll;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::os::raw::{c_int, c_uint};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::time::Instant;

pub(crate) const MAX_MIB: u64 = 64;
pub(crate) const MAX_PINGS: u64 = 1000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;
const HEADER_MAGIC: &[u8; 8] = b"ELBENCH1";
const READY: &[u8; 4] = b"RDY1";
const CHUNK: usize = 16 * 1024;

pub(crate) struct Config {
    pub(crate) bind: Ipv4Addr,
    pub(crate) peer: Ipv4Addr,
    pub(crate) port: u16,
    pub(crate) session: [u8; 32],
    pub(crate) mib: u64,
    pub(crate) pings: u64,
}

pub(crate) fn address(value: &str) -> Result<Ipv4Addr, String> {
    let address: Ipv4Addr = value
        .parse()
        .map_err(|_| "expected a numeric IPv4 address")?;
    let last = address.octets()[3];
    if !(address.is_private() || address.is_loopback() || address.is_link_local())
        || matches!(last, 0 | 255)
    {
        return Err(
            "peer networking requires a private, link-local, or loopback IPv4 host address".into(),
        );
    }
    Ok(address)
}

pub(crate) fn session(value: &str) -> Result<[u8; 32], String> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("'--session' must contain exactly 32 hexadecimal characters".into());
    }
    let mut session = [0; 32];
    session.copy_from_slice(value.to_ascii_lowercase().as_bytes());
    Ok(session)
}

impl Config {
    fn header(&self) -> [u8; 56] {
        let mut header = [0; 56];
        header[..8].copy_from_slice(HEADER_MAGIC);
        header[8..40].copy_from_slice(&self.session);
        header[40..48].copy_from_slice(&self.mib.to_be_bytes());
        header[48..].copy_from_slice(&self.pings.to_be_bytes());
        header
    }
}

#[repr(C)]
struct SockAddr {
    family: u16,
    port: u16,
    address: u32,
    zero: [u8; 8],
}

impl SockAddr {
    fn new(ip: Ipv4Addr, port: u16) -> Self {
        Self {
            family: 2,
            port: port.to_be(),
            address: u32::from_ne_bytes(ip.octets()),
            zero: [0; 8],
        }
    }
}

extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const SockAddr, len: c_uint) -> c_int;
    fn connect(fd: c_int, addr: *const SockAddr, len: c_uint) -> c_int;
}

#[cfg(target_arch = "mips")]
const SOCKET_FLAGS: c_int = 2 | 0x80 | 0o2000000; // SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC
#[cfg(not(target_arch = "mips"))]
const SOCKET_FLAGS: c_int = 1 | 0o4000 | 0o2000000;

#[cfg(target_arch = "mips")]
const EINPROGRESS: i32 = 150;
#[cfg(not(target_arch = "mips"))]
const EINPROGRESS: i32 = 115;

fn dial(config: &Config, deadline: Deadline) -> Result<TcpStream, String> {
    if !cfg!(all(
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "mips"
        )
    )) {
        return Err("peer TCP source binding is unavailable for this Linux target ABI".into());
    }
    deadline.check()?;
    // AF_INET with a nonblocking close-on-exec TCP socket and two sockaddr_in arguments.
    let fd = unsafe { socket(2, SOCKET_FLAGS, 0) };
    if fd < 0 {
        return Err(format!(
            "cannot open TCP socket: {}",
            io::Error::last_os_error()
        ));
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let length = std::mem::size_of::<SockAddr>() as c_uint;
    if unsafe { bind(fd.as_raw_fd(), &SockAddr::new(config.bind, 0), length) } < 0 {
        return Err(format!(
            "cannot bind source {}: {}",
            config.bind,
            io::Error::last_os_error()
        ));
    }
    let result = unsafe {
        connect(
            fd.as_raw_fd(),
            &SockAddr::new(config.peer, config.port),
            length,
        )
    };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(EINPROGRESS) {
            return Err(format!("cannot connect to peer: {error}"));
        }
        linux_poll::wait(fd.as_raw_fd(), linux_poll::WRITE, deadline)?;
    }
    let stream = TcpStream::from(fd);
    if let Some(error) = stream
        .take_error()
        .map_err(|error| format!("cannot check connection: {error}"))?
    {
        return Err(format!("cannot connect to peer: {error}"));
    }
    Ok(stream)
}

fn accept(listener: &TcpListener, peer: Ipv4Addr, deadline: Deadline) -> Result<TcpStream, String> {
    loop {
        deadline.check()?;
        match listener.accept() {
            Ok((stream, SocketAddr::V4(source))) if *source.ip() == peer => return Ok(stream),
            Ok(_) => return Err("connection came from a different address than '--peer'".into()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                linux_poll::wait(listener.as_raw_fd(), linux_poll::READ, deadline)?;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
            Err(error) => return Err(format!("cannot accept peer: {error}")),
        }
    }
}

struct Wire(TcpStream, Deadline);

impl Wire {
    fn new(stream: TcpStream, deadline: Deadline) -> Result<Self, String> {
        stream
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        stream
            .set_nodelay(true)
            .map_err(|error| error.to_string())?;
        Ok(Self(stream, deadline))
    }

    fn read(&mut self, mut target: &mut [u8]) -> Result<(), String> {
        while !target.is_empty() {
            self.1.check()?;
            match self.0.read(target) {
                Ok(0) => return Err("peer closed before completing its transfer".into()),
                Ok(count) => target = &mut target[count..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    linux_poll::wait(self.0.as_raw_fd(), linux_poll::READ, self.1)?
                }
                Err(error) => return Err(format!("peer read failed: {error}")),
            }
        }
        self.1.check()?;
        Ok(())
    }

    fn write(&mut self, mut content: &[u8]) -> Result<(), String> {
        while !content.is_empty() {
            self.1.check()?;
            match self.0.write(content) {
                Ok(0) => return Err("peer TCP write returned zero bytes".into()),
                Ok(count) => content = &content[count..],
                Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    linux_poll::wait(self.0.as_raw_fd(), linux_poll::WRITE, self.1)?
                }
                Err(error) => return Err(format!("peer write failed: {error}")),
            }
        }
        self.1.check()?;
        Ok(())
    }
}

fn payload() -> [u8; CHUNK] {
    let mut data = [0; CHUNK];
    let mut seed = 0x1234_5678u32;
    for byte in &mut data {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        *byte = seed as u8;
    }
    data
}

pub(crate) fn server(config: &Config, deadline: Deadline) -> Result<(), String> {
    let listener = TcpListener::bind(SocketAddrV4::new(config.bind, config.port))
        .map_err(|error| format!("cannot listen on {}:{}: {error}", config.bind, config.port))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    println!(
        "Peer listener: {}",
        listener.local_addr().map_err(|error| error.to_string())?
    );
    let mut wire = Wire::new(accept(&listener, config.peer, deadline)?, deadline)?;
    drop(listener);
    let mut header = [0; 56];
    wire.read(&mut header)?;
    if header != config.header() {
        return Err("peer protocol, session, or budgets do not match".into());
    }
    wire.write(READY)?;
    for index in 0..config.pings {
        let mut ping = [0; 8];
        wire.read(&mut ping)?;
        if ping != index.to_be_bytes() {
            return Err("peer sent an unexpected latency probe".into());
        }
        wire.write(&ping)?;
    }
    let expected = payload();
    let mut received = [0; CHUNK];
    let bytes = config.mib * MIB;
    let start = Instant::now();
    for _ in 0..bytes / CHUNK as u64 {
        wire.read(&mut received)?;
        if received != expected {
            return Err("peer throughput payload mismatch".into());
        }
    }
    let elapsed = start.elapsed();
    wire.write(&bytes.to_be_bytes())?;
    println!("Peer throughput (TCP receiver, verified payload)");
    println!("Bytes verified: {bytes}");
    println!("Elapsed: {:.6} s", elapsed.as_secs_f64());
    println!(
        "Throughput: {:.2} MiB/s",
        config.mib as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}

pub(crate) fn client(config: &Config, deadline: Deadline) -> Result<(), String> {
    let mut wire = Wire::new(dial(config, deadline)?, deadline)?;
    wire.write(&config.header())?;
    let mut ready = [0; 4];
    wire.read(&mut ready)?;
    if &ready != READY {
        return Err("peer did not acknowledge the session".into());
    }
    let mut samples = Vec::with_capacity(config.pings as usize);
    for index in 0..config.pings {
        let ping = index.to_be_bytes();
        let mut echo = [0; 8];
        let start = Instant::now();
        wire.write(&ping)?;
        wire.read(&mut echo)?;
        if echo != ping {
            return Err("peer latency echo mismatch".into());
        }
        samples.push(start.elapsed());
    }
    let expected = payload();
    let bytes = config.mib * MIB;
    let start = Instant::now();
    for _ in 0..bytes / CHUNK as u64 {
        wire.write(&expected)?;
    }
    let mut count = [0; 8];
    wire.read(&mut count)?;
    let elapsed = start.elapsed();
    if u64::from_be_bytes(count) != bytes {
        return Err("peer did not verify the requested byte count".into());
    }
    samples.sort_unstable();
    let mean = samples.iter().map(|time| time.as_secs_f64()).sum::<f64>() / samples.len() as f64;
    let p95 = samples[(95 * samples.len() + 99) / 100 - 1].as_secs_f64();
    println!(
        "Peer latency (application TCP round trip)\nProbes: {}",
        samples.len()
    );
    println!(
        "RTT min / mean / p95: {:.3} / {:.3} / {:.3} ms",
        samples[0].as_secs_f64() * 1000.0,
        mean * 1000.0,
        p95 * 1000.0
    );
    println!("Peer throughput (TCP sender, including final acknowledgment)");
    println!("Bytes verified by peer: {bytes}");
    println!("Elapsed: {:.6} s", elapsed.as_secs_f64());
    println!(
        "Throughput: {:.2} MiB/s",
        config.mib as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}

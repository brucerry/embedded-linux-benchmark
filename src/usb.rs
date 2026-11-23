use crate::common::Deadline;
use crate::peripheral;
use std::ffi::c_void;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_BYTES: u64 = 4096;
pub(crate) const MAX_BYTES: u64 = 65_536;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;
const CHUNK: usize = 4096;
const USB_DEVICE_MAJOR: u64 = 189;

pub(crate) fn endpoint(value: &str, input: bool) -> Result<u8, String> {
    let parsed = match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(value) => u8::from_str_radix(value, 16),
        None => value.parse(),
    };
    let range = if input { 0x81..=0x8f } else { 0x01..=0x0f };
    parsed
        .ok()
        .filter(|value| range.contains(value))
        .ok_or_else(|| {
            format!(
                "USB {} endpoint must be in 0x{:02x}–0x{:02x}",
                if input { "IN" } else { "OUT" },
                range.start(),
                range.end()
            )
        })
}

pub(crate) struct Config<'a> {
    pub(crate) device: &'a Path,
    pub(crate) interface: u32,
    pub(crate) input: u8,
    pub(crate) output: u8,
    pub(crate) bytes: usize,
}

#[repr(C)]
struct BulkTransfer {
    endpoint: u32,
    length: u32,
    timeout_ms: u32,
    data: *mut c_void,
}

const BULK: u32 = peripheral::ioctl_read_write::<BulkTransfer>(b'U', 2);
const CLAIM: u32 = peripheral::ioctl_read::<u32>(b'U', 15);
const RELEASE: u32 = peripheral::ioctl_read::<u32>(b'U', 16);

fn attribute(path: &Path, name: &str) -> Result<String, String> {
    let mut content = String::new();
    File::open(path.join(name))
        .and_then(|file| file.take(129).read_to_string(&mut content))
        .map_err(|error| format!("cannot read USB {name}: {error}"))?;
    if content.len() > 128 {
        return Err(format!("invalid USB {name} attribute"));
    }
    Ok(content.trim().to_owned())
}

fn verify_interface(root: &Path, config: &Config<'_>) -> Result<String, String> {
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid USB kernel device name")?;
    let configuration = attribute(root, "bConfigurationValue")?
        .parse::<u8>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or("USB device has no active configuration")?;
    let interface = root.join(format!("{name}:{configuration}.{}", config.interface));
    let number = attribute(&interface, "bInterfaceNumber")?;
    if u32::from_str_radix(&number, 16).ok() != Some(config.interface) {
        return Err("USB kernel interface number does not match the fixture".into());
    }
    match fs::symlink_metadata(interface.join("driver")) {
        Ok(_) => {
            return Err(
                "USB fixture interface is owned by a kernel driver; it will not be detached".into(),
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(format!("cannot confirm USB interface ownership: {error}")),
    }
    let alternate = attribute(&interface, "bAlternateSetting")?;
    if alternate.parse::<u8>().is_err() {
        return Err("invalid current USB alternate setting".into());
    }
    for address in [config.output, config.input] {
        let endpoint = interface.join(format!("ep_{address:02x}"));
        if attribute(&endpoint, "type")? != "Bulk"
            || u8::from_str_radix(&attribute(&endpoint, "bEndpointAddress")?, 16).ok()
                != Some(address)
        {
            return Err(format!(
                "USB endpoint 0x{address:02x} is not a bulk endpoint on the current interface"
            ));
        }
    }
    Ok(alternate)
}

fn node_numbers(path: &Path) -> Result<(u16, u16), String> {
    let relative = path
        .strip_prefix("/dev/bus/usb")
        .map_err(|_| "expected /dev/bus/usb/BBB/DDD")?;
    let pieces = relative.iter().collect::<Vec<_>>();
    if let [bus, device] = pieces.as_slice() {
        let triple = |name: &std::ffi::OsStr| {
            let text = name.to_str()?;
            if text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_digit()) {
                text.parse::<u16>().ok().filter(|number| *number > 0)
            } else {
                None
            }
        };
        if let (Some(bus), Some(device)) = (triple(bus), triple(device)) {
            return Ok((bus, device));
        }
    }
    Err("expected /dev/bus/usb/BBB/DDD with explicit bus and device numbers".into())
}

fn open(config: &Config<'_>) -> Result<(File, String), String> {
    peripheral::supported()?;
    let path = config
        .device
        .canonicalize()
        .map_err(|error| format!("cannot resolve USB fixture: {error}"))?;
    let (bus, device) = node_numbers(&path)?;
    let (identity, root) = peripheral::char_identity(&path, "USB fixture")?;
    let subsystem = fs::read_link(root.join("subsystem"))
        .map_err(|error| format!("cannot confirm USB subsystem: {error}"))?;
    if ((identity.rdev() >> 8) & 0xfff) != USB_DEVICE_MAJOR
        || subsystem.file_name().and_then(|s| s.to_str()) != Some("usb")
        || attribute(&root, "busnum")?.parse::<u16>().ok() != Some(bus)
        || attribute(&root, "devnum")?.parse::<u16>().ok() != Some(device)
    {
        return Err("USB device node does not match its kernel bus and device records".into());
    }
    let alternate = verify_interface(&root, config)?;
    Ok((
        peripheral::open_identified(&path, "USB fixture", &identity, true)?,
        alternate,
    ))
}

struct Interface<'a> {
    file: &'a File,
    number: u32,
    claimed: bool,
}

impl<'a> Interface<'a> {
    fn claim(file: &'a File, number: u32) -> Result<Self, String> {
        let mut interface = number;
        // The ioctl takes a pointer to the exact interface, without a detach flag.
        unsafe { peripheral::ioctl_ptr(file, CLAIM, &mut interface) }
            .map_err(|error| format!("cannot claim the dedicated USB interface: {error}"))?;
        Ok(Self {
            file,
            number,
            claimed: true,
        })
    }

    fn release(&mut self) -> Result<(), String> {
        if self.claimed {
            unsafe { peripheral::ioctl_ptr(self.file, RELEASE, &mut self.number) }
                .map_err(|error| format!("cannot release the USB interface: {error}"))?;
            self.claimed = false;
        }
        Ok(())
    }
}

impl Drop for Interface<'_> {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

fn bulk(file: &File, endpoint: u8, data: &mut [u8], deadline: Deadline) -> Result<usize, String> {
    let time = deadline.remaining()?;
    let timeout_ms = ((time.as_nanos() + 999_999) / 1_000_000).min(u32::MAX as u128) as u32;
    let mut transfer = BulkTransfer {
        endpoint: u32::from(endpoint),
        length: data.len() as u32,
        timeout_ms,
        data: data.as_mut_ptr().cast(),
    };
    // The checked usbfs node receives one synchronous transfer; the buffer remains live.
    unsafe { peripheral::ioctl_ptr(file, BULK, &mut transfer) }
        .map_err(|error| format!("USB endpoint 0x{endpoint:02x} transfer failed: {error}"))
}

fn loopback(
    config: &Config<'_>,
    deadline: Deadline,
    mut transfer: impl FnMut(u8, &mut [u8]) -> Result<usize, String>,
) -> Result<Duration, String> {
    let mut tx = [0; CHUNK];
    let mut rx = [0; CHUNK];
    let mut seed = 0x1234_5678u32;
    let start = Instant::now();
    for offset in (0..config.bytes).step_by(CHUNK) {
        let size = CHUNK.min(config.bytes - offset);
        for byte in &mut tx[..size] {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            *byte = seed as u8;
        }
        deadline.check()?;
        let written = transfer(config.output, &mut tx[..size])?;
        if written != size {
            return Err(format!("USB bulk OUT returned {written} of {size} bytes"));
        }
        deadline.check()?;
        let received = transfer(config.input, &mut rx[..size])?;
        if received != size {
            return Err(format!("USB bulk IN returned {received} of {size} bytes"));
        }
        if tx[..size] != rx[..size] {
            return Err(format!(
                "USB loopback payload mismatch in chunk starting at byte {offset}"
            ));
        }
    }
    deadline.check()?;
    Ok(start.elapsed())
}

pub(crate) fn run(config: &Config<'_>, deadline: Deadline) -> Result<(), String> {
    deadline.check()?;
    let (file, alternate) = open(config)?;
    deadline.check()?;
    let mut interface = Interface::claim(&file, config.interface)?;
    let measured = loopback(config, deadline, |endpoint, data| {
        bulk(&file, endpoint, data, deadline)
    });
    let release = interface.release();
    let elapsed = measured?;
    release?;
    println!("USB throughput (verified bulk loopback)");
    println!("Fixture: {}", config.device.display());
    println!("Interface: {}, alternate {alternate}", config.interface);
    println!(
        "Endpoints: OUT 0x{:02x}, IN 0x{:02x}",
        config.output, config.input
    );
    println!("Bytes verified: {}", config.bytes);
    println!("Elapsed: {:.6} s", elapsed.as_secs_f64());
    println!(
        "Throughput: {:.2} B/s",
        config.bytes as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inert_sysfs_checks_bulk_endpoints_and_unbound_interface() {
        let path = std::env::temp_dir().join(format!("benchmark-usb-{}", std::process::id()));
        let interface = path.join(format!(
            "{}:1.0",
            path.file_name().unwrap().to_str().unwrap()
        ));
        fs::create_dir_all(interface.join("ep_01")).unwrap();
        fs::create_dir(interface.join("ep_81")).unwrap();
        fs::write(path.join("bConfigurationValue"), "1\n").unwrap();
        fs::write(interface.join("bInterfaceNumber"), "00\n").unwrap();
        fs::write(interface.join("bAlternateSetting"), " 0\n").unwrap();
        for (dir, address) in [("ep_01", "01"), ("ep_81", "81")] {
            fs::write(interface.join(dir).join("type"), "Bulk\n").unwrap();
            fs::write(interface.join(dir).join("bEndpointAddress"), address).unwrap();
        }
        let config = Config {
            device: Path::new("/dev/bus/usb/001/011"),
            interface: 0,
            input: 0x81,
            output: 1,
            bytes: 16,
        };
        let valid = verify_interface(&path, &config);
        fs::write(interface.join("ep_81/type"), "Interrupt\n").unwrap();
        let wrong_type = verify_interface(&path, &config);
        fs::create_dir(interface.join("driver")).unwrap();
        let bound = verify_interface(&path, &config);
        fs::remove_dir_all(path).unwrap();
        assert_eq!(valid.unwrap(), "0");
        assert!(wrong_type.unwrap_err().contains("not a bulk endpoint"));
        assert!(bound.unwrap_err().contains("owned by a kernel driver"));
    }

    #[test]
    fn loopback_requires_both_directions_and_exact_echo() {
        let config = Config {
            device: Path::new("/dev/bus/usb/001/011"),
            interface: 0,
            input: 0x81,
            output: 1,
            bytes: 4101,
        };
        let deadline = || Deadline::after(Duration::from_secs(3));
        let mut response = Vec::new();
        let mut calls = Vec::new();
        loopback(&config, deadline(), |ep, data| {
            calls.push((ep, data.len()));
            if ep == config.output {
                response = data.to_vec();
            } else {
                data.copy_from_slice(&response);
            }
            Ok(data.len())
        })
        .unwrap();
        assert_eq!(calls, [(1, 4096), (0x81, 4096), (1, 5), (0x81, 5)]);
        assert!(loopback(&config, deadline(), |_, _| Ok(1))
            .unwrap_err()
            .contains("bulk OUT"));
        assert!(loopback(&config, deadline(), |_, data| Ok(data.len()))
            .unwrap_err()
            .contains("payload mismatch"));
        assert!(
            loopback(&config, Deadline::after(Duration::ZERO), |_, _| panic!(
                "expired transfer started"
            ))
            .is_err()
        );
        assert!(node_numbers(Path::new("/dev/bus/usb/001/011")).is_ok());
        assert!(node_numbers(Path::new("/dev/bus/usb/001/011/01")).is_err());
        assert_eq!(endpoint("0x81", true), Ok(0x81));
        assert!(endpoint("0x81", false).is_err());
    }
}

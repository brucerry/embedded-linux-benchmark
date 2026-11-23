// Shared Linux device admission and Linux UAPI ioctl conventions.
use crate::common::Deadline;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::os::raw::c_ulong;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SUPPORTED: bool = cfg!(all(
    target_os = "linux",
    any(target_env = "gnu", target_env = "musl"),
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "mips"
    )
));
pub(crate) const EBUSY: i32 = 16;
pub(crate) const EINVAL: i32 = 22;
pub(crate) const ENOTTY: i32 = 25;

#[cfg(target_env = "musl")]
type Request = std::os::raw::c_int;
#[cfg(not(target_env = "musl"))]
type Request = c_ulong;

extern "C" {
    fn ioctl(fd: std::os::raw::c_int, request: Request, ...) -> std::os::raw::c_int;
}

// Linux asm-generic/{ioctl,fcntl}.h, the Arm fcntl overrides, and
// arch/mips/include/uapi/asm/{ioctl,fcntl}.h.
const READ: u32 = 2;
#[cfg(target_arch = "mips")]
const WRITE: u32 = 4;
#[cfg(not(target_arch = "mips"))]
const WRITE: u32 = 1;
#[cfg(target_arch = "mips")]
const DIR_SHIFT: u32 = 29;
#[cfg(not(target_arch = "mips"))]
const DIR_SHIFT: u32 = 30;
#[cfg(target_arch = "mips")]
const NONBLOCK_NOCTTY: i32 = 0x0080 | 0x0800;
#[cfg(not(target_arch = "mips"))]
const NONBLOCK_NOCTTY: i32 = 0o4000 | 0o400;
#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const NOFOLLOW: i32 = 0o100000;
#[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
const NOFOLLOW: i32 = 0o400000;

pub(crate) const fn ioctl_read<T>(kind: u8, number: u8) -> u32 {
    ioctl_code::<T>(READ, kind, number)
}

pub(crate) const fn ioctl_write<T>(kind: u8, number: u8) -> u32 {
    ioctl_code::<T>(WRITE, kind, number)
}

pub(crate) const fn ioctl_read_write<T>(kind: u8, number: u8) -> u32 {
    ioctl_code::<T>(READ | WRITE, kind, number)
}

const fn ioctl_code<T>(direction: u32, kind: u8, number: u8) -> u32 {
    (direction << DIR_SHIFT)
        | ((std::mem::size_of::<T>() as u32) << 16)
        | ((kind as u32) << 8)
        | number as u32
}

pub(crate) fn supported() -> Result<(), String> {
    if SUPPORTED {
        Ok(())
    } else {
        Err("device ioctls are unavailable for this Linux target ABI".into())
    }
}

// Safety: the request must take a pointer to a UAPI object with exactly T's layout,
// and all pointers carried inside it must remain valid throughout the ioctl.
pub(crate) unsafe fn ioctl_ptr<T>(file: &File, request: u32, value: &mut T) -> io::Result<usize> {
    if !SUPPORTED {
        return Err(io::ErrorKind::Unsupported.into());
    }
    let result = unsafe { ioctl(file.as_raw_fd(), request as Request, value as *mut T) };
    ioctl_result(result)
}

pub(crate) fn ioctl_i2c_slave(file: &File, address: u8) -> io::Result<usize> {
    if !SUPPORTED {
        return Err(io::ErrorKind::Unsupported.into());
    }
    // I2C_SLAVE uses an unsigned long scalar argument.
    let result = unsafe { ioctl(file.as_raw_fd(), 0x0703 as Request, c_ulong::from(address)) };
    ioctl_result(result)
}

fn ioctl_result(result: i32) -> io::Result<usize> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result as usize)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    I2c,
    Spi,
    Gpio,
    #[cfg(all(
        target_pointer_width = "64",
        any(target_arch = "x86_64", target_arch = "aarch64"),
        any(target_env = "gnu", target_env = "musl")
    ))]
    Video,
}

impl Kind {
    fn subsystem(self) -> &'static str {
        match self {
            Self::I2c => "i2c-dev",
            Self::Spi => "spidev",
            Self::Gpio => "gpio",
            #[cfg(all(
                target_pointer_width = "64",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                any(target_env = "gnu", target_env = "musl")
            ))]
            Self::Video => "video4linux",
        }
    }

    fn valid_name(self, name: &str) -> bool {
        fn digits(s: &str) -> bool {
            !s.is_empty() && s.bytes().all(|byte| byte.is_ascii_digit())
        }
        match self {
            Self::I2c => name.strip_prefix("i2c-").map_or(false, digits),
            Self::Gpio => name.strip_prefix("gpiochip").map_or(false, digits),
            #[cfg(all(
                target_pointer_width = "64",
                any(target_arch = "x86_64", target_arch = "aarch64"),
                any(target_env = "gnu", target_env = "musl")
            ))]
            Self::Video => name.strip_prefix("video").map_or(false, digits),
            Self::Spi => name
                .strip_prefix("spidev")
                .and_then(|n| n.split_once('.'))
                .map_or(false, |(bus, chip)| digits(bus) && digits(chip)),
        }
    }
}

pub(crate) fn open_device(path: &Path, kind: Kind, writable: bool) -> Result<File, String> {
    supported()?;
    let path = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve {} device: {error}", kind.subsystem()))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| kind.valid_name(name))
        .filter(|_| path.parent() == Some(Path::new("/dev")))
        .ok_or_else(|| format!("expected a {} device node in /dev", kind.subsystem()))?;
    let (expected, sys_device) = char_identity(&path, name)?;
    let subsystem = fs::read_link(sys_device.join("subsystem"))
        .map_err(|error| format!("cannot confirm {name} subsystem in sysfs: {error}"))?;
    if sys_device.file_name().and_then(|s| s.to_str()) != Some(name)
        || subsystem.file_name().and_then(|s| s.to_str()) != Some(kind.subsystem())
    {
        return Err(format!(
            "{name} does not match the kernel's {} device",
            kind.subsystem()
        ));
    }

    open_identified(&path, name, &expected, writable)
}

pub(crate) fn char_identity(path: &Path, name: &str) -> Result<(Metadata, PathBuf), String> {
    let metadata = fs::metadata(path).map_err(|error| format!("cannot inspect {name}: {error}"))?;
    if !metadata.file_type().is_char_device() {
        return Err(format!("{name} must be a character device"));
    }
    // Linux stat device numbers encode the extended minor in bits 20..31.
    let device = metadata.rdev();
    let major = (device >> 8) & 0xfff;
    let minor = (device & 0xff) | ((device >> 12) & 0xfff00);
    let sys_device = fs::canonicalize(format!("/sys/dev/char/{major}:{minor}"))
        .map_err(|error| format!("cannot confirm {name} in sysfs: {error}"))?;
    Ok((metadata, sys_device))
}

pub(crate) fn open_identified(
    path: &Path,
    name: &str,
    expected: &Metadata,
    writable: bool,
) -> Result<File, String> {
    supported()?;
    let file = OpenOptions::new()
        .read(true)
        .write(writable)
        .custom_flags(NOFOLLOW | NONBLOCK_NOCTTY)
        .open(path)
        .map_err(|error| format!("cannot open {name}: {error}"))?;
    let actual = file
        .metadata()
        .map_err(|error| format!("cannot inspect open device: {error}"))?;
    if !actual.file_type().is_char_device()
        || (actual.dev(), actual.ino(), actual.rdev())
            != (expected.dev(), expected.ino(), expected.rdev())
    {
        return Err(format!("{name} changed during open"));
    }
    Ok(file)
}

pub(crate) fn measure_reads(
    samples: u64,
    deadline: Deadline,
    mut read: impl FnMut() -> Result<(), String>,
) -> Result<Duration, String> {
    let mut measured = Duration::ZERO;
    for _ in 0..samples {
        deadline.check()?;
        let start = Instant::now();
        let result = read();
        measured += start.elapsed();
        result?;
    }
    deadline.check()?;
    Ok(measured)
}

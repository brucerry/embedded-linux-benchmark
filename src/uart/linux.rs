use crate::common::Deadline;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
#[cfg(target_env = "gnu")]
use std::os::raw::c_ulong;
use std::os::raw::{c_int, c_uint};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

// Linux UAPI asm*/{fcntl,ioctls,termbits}.h. These are kernel termios
// layouts, independent of the libc termios struct. MIPS uses the same layout
// in both byte orders.
#[cfg(target_arch = "mips")]
const ABI: TtyAbi = TtyAbi {
    get: 0x540d,
    set: 0x540e,
    open_flags: 0x0800 | 0x0080, // O_NOCTTY | O_NONBLOCK
    vmin: 4,
};
#[cfg(not(target_arch = "mips"))]
const ABI: TtyAbi = TtyAbi {
    get: 0x5401,
    set: 0x5402,
    open_flags: 0o400 | 0o4000, // O_NOCTTY | O_NONBLOCK
    vmin: 6,
};
#[cfg(target_arch = "mips")]
const NCCS: usize = 23;
#[cfg(not(target_arch = "mips"))]
const NCCS: usize = 19;

#[cfg(any(target_arch = "arm", target_arch = "aarch64"))]
const NO_FOLLOW: c_int = 0o100000;
#[cfg(not(any(target_arch = "arm", target_arch = "aarch64")))]
const NO_FOLLOW: c_int = 0o400000;

const VTIME: usize = 5;
const CBAUD: c_uint = 0x100f;
const HUPCL: c_uint = 0x0400;
const UART_CONTROL: c_uint = 0x1002 | 0x0030 | 0x0080 | 0x0800; // B115200 | CS8 | CREAD | CLOCAL
const CHUNK: usize = 512;

#[cfg(target_env = "gnu")]
type Request = c_ulong;
#[cfg(target_env = "musl")]
type Request = c_int;

struct TtyAbi {
    get: Request,
    set: Request,
    open_flags: c_int,
    vmin: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Termios {
    input: c_uint,
    output: c_uint,
    control: c_uint,
    local: c_uint,
    line: u8,
    chars: [u8; NCCS],
}

const _: [(); 16 + 1 + NCCS] = [(); std::mem::size_of::<Termios>()];

extern "C" {
    fn ioctl(fd: c_int, request: Request, ...) -> c_int;
}

impl Termios {
    fn get(file: &File) -> io::Result<Self> {
        let mut settings = Self::default();
        // The kernel writes one termios to the initialized, correctly sized buffer.
        let result = unsafe { ioctl(file.as_raw_fd(), ABI.get, &mut settings as *mut Self) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(settings)
    }

    fn set(&self, file: &File) -> io::Result<()> {
        // TCSETS applies immediately, with no output drain, and reads one termios.
        let result = unsafe { ioctl(file.as_raw_fd(), ABI.set, self as *const Self) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

struct Settings<'a> {
    file: &'a File,
    original: Termios,
    restored: bool,
}

impl Settings<'_> {
    fn restore(&mut self) -> Result<(), String> {
        self.original
            .set(self.file)
            .map_err(|error| format!("could not restore tty settings: {error}"))?;
        if Termios::get(self.file)
            .map_err(|error| format!("could not verify restored tty settings: {error}"))?
            != self.original
        {
            return Err("tty did not restore the original settings".into());
        }
        self.restored = true;
        Ok(())
    }
}

impl Drop for Settings<'_> {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.original.set(self.file);
        }
    }
}

fn device_numbers(major: u64, minor: u64) -> u64 {
    ((major & 0xfff) << 8) | (minor & 0xff) | ((minor & 0xfff00) << 12)
}

fn parse_device_numbers(value: &str) -> Option<u64> {
    let (major, minor) = value.split_once(':')?;
    let (major, minor) = (major.parse().ok()?, minor.parse().ok()?);
    Some(device_numbers(major, minor))
}

fn numbered(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn proc_file(path: &str) -> Result<String, String> {
    fs::read_to_string(path)
        .map_err(|error| format!("cannot verify UART safety using {path}: {error}"))
}

fn reject_process_terminals(device: u64) -> Result<(), String> {
    for fd in 0..=2 {
        match fs::metadata(format!("/proc/self/fd/{fd}")) {
            Ok(info) if info.file_type().is_char_device() && info.rdev() == device => {
                return Err("UART device is a standard stream".into());
            }
            Ok(_) => (),
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => return Err(format!("cannot inspect standard stream {fd}: {error}")),
        }
    }
    let stat = proc_file("/proc/thread-self/stat")?;
    // After the parenthesized command, the fifth field is the encoded tty number.
    let tty_number = stat
        .rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(4))
        .and_then(|field| field.parse::<i32>().ok())
        .ok_or("cannot verify the controlling terminal in /proc/thread-self/stat")?;
    if tty_number != 0 && u64::from(tty_number as u32) == device {
        return Err("UART device is the controlling terminal".into());
    }
    Ok(())
}

fn select_device(path: &Path) -> Result<(PathBuf, Metadata), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("cannot resolve UART device: {error}"))?;
    let info =
        fs::metadata(&path).map_err(|error| format!("cannot inspect UART device: {error}"))?;
    if !info.file_type().is_char_device() {
        return Err("UART requires a serial character device".into());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let device = info.rdev();
    let is_pts = path.parent() == Some(Path::new("/dev/pts"))
        && numbered(name)
        && (136..=143).contains(&((device >> 8) & 0xfff));
    let is_serial = path.parent() == Some(Path::new("/dev"))
        && [
            "ttyS", "ttyUSB", "ttyACM", "ttyAMA", "ttyTHS", "ttymxc", "ttySTM", "ttyMSM", "ttyO",
            "ttyPS", "ttyUL", "ttySAC",
        ]
        .iter()
        .any(|prefix| name.strip_prefix(prefix).map_or(false, numbered));
    if !is_pts && !is_serial {
        return Err("UART requires a recognized serial tty or PTY slave; console and standard device nodes are refused".into());
    }
    reject_process_terminals(device)?;
    if is_serial {
        let consoles = proc_file("/proc/consoles")?;
        let cmdline = proc_file("/proc/cmdline")?;
        let active_console = consoles.lines().any(|line| {
            let mut fields = line.split_whitespace();
            fields.next() == Some(name)
                || fields.last().and_then(parse_device_numbers) == Some(device)
        });
        let boot_console = cmdline.split_whitespace().any(|arg| {
            arg.strip_prefix("console=").map_or(false, |value| {
                let console = value.split(',').next().unwrap_or("");
                console.strip_prefix("/dev/").unwrap_or(console) == name
            })
        });
        if active_console || boot_console {
            return Err("UART device is configured as a Linux console".into());
        }
        let number = fs::read_to_string(format!("/sys/class/tty/{name}/dev"))
            .map_err(|error| format!("cannot verify the kernel's UART device number: {error}"))?;
        if parse_device_numbers(number.trim()) != Some(device) {
            return Err("UART device number differs from the kernel tty record".into());
        }
    }
    Ok((path, info))
}

fn transfer(mut file: &File, bytes: usize, deadline: Deadline) -> Result<Duration, String> {
    let start = Instant::now();
    let mut outgoing = [0; CHUNK];
    let mut incoming = [0; CHUNK];
    let mut pattern = 0x1234_5678u32;
    for offset in (0..bytes).step_by(CHUNK) {
        let length = CHUNK.min(bytes - offset);
        for byte in &mut outgoing[..length] {
            pattern ^= pattern << 13;
            pattern ^= pattern >> 17;
            pattern ^= pattern << 5;
            *byte = pattern as u8;
        }
        let mut sent = 0;
        let mut received = 0;
        while received < length {
            deadline.check()?;
            let mut progressed = false;
            if sent < length {
                match file.write(&outgoing[sent..length]) {
                    Ok(count) => {
                        sent += count;
                        progressed = count != 0;
                    }
                    Err(error) if retry(&error) => (),
                    Err(error) => return Err(format!("UART write failed: {error}")),
                }
            }
            if received < sent {
                match file.read(&mut incoming[..sent - received]) {
                    Ok(count) => {
                        if incoming[..count] != outgoing[received..received + count] {
                            return Err(format!(
                                "UART loopback payload mismatch near byte {}",
                                offset + received
                            ));
                        }
                        received += count;
                        progressed |= count != 0;
                    }
                    Err(error) if retry(&error) => (),
                    Err(error) => return Err(format!("UART read failed: {error}")),
                }
            }
            if !progressed {
                thread::sleep(deadline.remaining()?.min(Duration::from_millis(1)));
            }
        }
    }
    deadline.check()?;
    Ok(start.elapsed())
}

fn retry(error: &io::Error) -> bool {
    matches!(error.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock)
}

pub(super) fn run(device: &Path, bytes: usize, deadline: Deadline) -> Result<(), String> {
    let (path, expected) = select_device(device)?;
    deadline.check()?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(ABI.open_flags | NO_FOLLOW)
        .open(&path)
        .map_err(|error| format!("cannot open UART device: {error}"))?;
    let actual = file
        .metadata()
        .map_err(|error| format!("cannot inspect open UART device: {error}"))?;
    if !actual.file_type().is_char_device()
        || (actual.dev(), actual.ino(), actual.rdev())
            != (expected.dev(), expected.ino(), expected.rdev())
    {
        return Err("UART device changed during open".into());
    }
    let original =
        Termios::get(&file).map_err(|error| format!("cannot read tty settings: {error}"))?;
    if original.line != 0 || original.control & CBAUD == 0 {
        return Err(
            "UART requires the standard tty discipline and a nonzero existing baud rate".into(),
        );
    }
    let mut configured = Termios {
        input: 0,
        output: 0,
        control: (original.control & HUPCL) | UART_CONTROL,
        local: 0,
        ..original
    };
    configured.chars[ABI.vmin] = 0;
    configured.chars[VTIME] = 0;
    deadline.check()?;
    let mut settings = Settings {
        file: &file,
        original,
        restored: false,
    };
    let result = (|| {
        configured
            .set(&file)
            .map_err(|error| format!("cannot configure tty: {error}"))?;
        if Termios::get(&file).map_err(|error| format!("cannot verify tty settings: {error}"))?
            != configured
        {
            return Err("tty did not accept 115200 8N1 raw settings".into());
        }
        transfer(&file, bytes, deadline)
    })();
    let elapsed = match (result, settings.restore()) {
        (Ok(elapsed), Ok(())) => elapsed,
        (Err(error), Ok(())) | (Ok(_), Err(error)) => return Err(error),
        (Err(error), Err(restore)) => return Err(format!("{error}; {restore}")),
    };
    let seconds = elapsed.as_secs_f64();
    crate::report::metric("UART loopback throughput", bytes as f64 / seconds, "B/s");
    report_line!("UART throughput (verified loopback, 115200 8N1)");
    report_line!("Device: {}", path.display());
    report_line!("Bytes verified: {bytes}");
    report_line!("Elapsed: {seconds:.6} s");
    report_line!("Throughput: {:.2} B/s", bytes as f64 / seconds);
    Ok(())
}

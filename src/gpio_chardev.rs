use crate::common::Deadline;
use crate::peripheral::{self, Kind};
use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;
use std::path::Path;
use std::time::Duration;

const USED: u64 = 1 << 0;
const ACTIVE_LOW: u64 = 1 << 1;
const INPUT: u64 = 1 << 2;
const OUTPUT: u64 = 1 << 3;
const NAME_SIZE: usize = 32;
const MAX_LINES: usize = 64;
const MAX_ATTRS: usize = 10;
const MAGIC: u8 = 0xb4;

#[repr(C)]
#[derive(Default)]
struct ChipInfo {
    name: [u8; NAME_SIZE],
    label: [u8; NAME_SIZE],
    lines: u32,
}

// Linux GPIO v2 uses __aligned_u64, including on 32-bit targets.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
struct Attribute {
    id: u32,
    padding: u32,
    value: u64, // Storage for the flags/values/debounce union.
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
struct ConfigAttribute {
    attr: Attribute,
    mask: u64,
}

#[repr(C, align(8))]
#[derive(Default)]
struct Config {
    flags: u64,
    num_attrs: u32,
    padding: [u32; 5],
    attrs: [ConfigAttribute; MAX_ATTRS],
}

#[repr(C, align(8))]
#[derive(Default)]
struct Info {
    name: [u8; NAME_SIZE],
    consumer: [u8; NAME_SIZE],
    offset: u32,
    num_attrs: u32,
    flags: u64,
    attrs: [Attribute; MAX_ATTRS],
    padding: [u32; 4],
}

#[repr(C, align(8))]
struct Request {
    offsets: [u32; MAX_LINES],
    consumer: [u8; NAME_SIZE],
    config: Config,
    num_lines: u32,
    event_buffer_size: u32,
    padding: [u32; 5],
    fd: i32,
}

impl Request {
    fn input_as_is(offset: u32, flags: u64) -> Self {
        let mut offsets = [0; MAX_LINES];
        offsets[0] = offset;
        let mut consumer = [0; NAME_SIZE];
        let label = b"eld-engine";
        consumer[..label.len()].copy_from_slice(label);
        Self {
            offsets,
            consumer,
            config: Config {
                flags: flags & ACTIVE_LOW,
                ..Config::default()
            },
            num_lines: 1,
            event_buffer_size: 0,
            padding: [0; 5],
            fd: -1,
        }
    }
}

#[repr(C, align(8))]
struct Values {
    bits: u64,
    mask: u64,
}

const _: [(); 16] = [(); std::mem::size_of::<Attribute>()];
const _: [(); 24] = [(); std::mem::size_of::<ConfigAttribute>()];
const _: [(); 272] = [(); std::mem::size_of::<Config>()];
const _: [(); 256] = [(); std::mem::size_of::<Info>()];
const _: [(); 592] = [(); std::mem::size_of::<Request>()];
const _: [(); 16] = [(); std::mem::size_of::<Values>()];

const _: [(); 68] = [(); std::mem::size_of::<ChipInfo>()];

const GET_CHIP_INFO: u32 = peripheral::ioctl_read::<ChipInfo>(MAGIC, 0x01);
const GET_INFO: u32 = peripheral::ioctl_read_write::<Info>(MAGIC, 0x05);
const GET_LINE: u32 = peripheral::ioctl_read_write::<Request>(MAGIC, 0x07);
const GET_VALUES: u32 = peripheral::ioctl_read_write::<Values>(MAGIC, 0x0e);

fn operation_error(operation: &str, error: io::Error) -> String {
    match error.raw_os_error() {
        Some(peripheral::ENOTTY) => {
            "GPIO chardev v2 is unavailable on this kernel/device (ENOTTY)".into()
        }
        Some(peripheral::EBUSY) => format!("GPIO line is busy: {error}"),
        _ => format!("{operation}: {error}"),
    }
}

trait Chip {
    type Line;
    fn flags(&self, offset: u32) -> Result<u64, String>;
    fn request_as_is(&self, offset: u32, flags: u64) -> Result<Self::Line, String>;
    fn active(&self, line: &Self::Line) -> Result<bool, String>;
}

impl Chip for File {
    type Line = File;

    fn flags(&self, offset: u32) -> Result<u64, String> {
        let mut info = Info {
            offset,
            ..Info::default()
        };
        // GPIO_V2_GET_LINEINFO_IOCTL takes a gpio_v2_line_info object.
        unsafe { peripheral::ioctl_ptr(self, GET_INFO, &mut info) }.map_err(|error| {
            if error.raw_os_error() == Some(peripheral::EINVAL) {
                // Linux 5.4 rejects unknown chardev requests with EINVAL.
                // The offset was checked against the chip line count.
                "GPIO chardev v2 line info is unavailable on this kernel/device (EINVAL)".into()
            } else {
                operation_error("cannot inspect GPIO line", error)
            }
        })?;
        Ok(info.flags)
    }

    fn request_as_is(&self, offset: u32, flags: u64) -> Result<File, String> {
        let mut request = Request::input_as_is(offset, flags);
        // GPIO_V2_GET_LINE_IOCTL takes a gpio_v2_line_request, returning an owned fd.
        unsafe { peripheral::ioctl_ptr(self, GET_LINE, &mut request) }
            .map_err(|error| operation_error("cannot request GPIO line as-is", error))?;
        if request.fd < 0 {
            return Err("GPIO kernel returned no line descriptor".into());
        }
        Ok(unsafe { File::from_raw_fd(request.fd) })
    }

    fn active(&self, line: &File) -> Result<bool, String> {
        let mut values = Values { bits: 0, mask: 1 };
        // GPIO_V2_LINE_GET_VALUES_IOCTL on the line fd takes gpio_v2_line_values.
        unsafe { peripheral::ioctl_ptr(line, GET_VALUES, &mut values) }
            .map_err(|error| operation_error("cannot read GPIO line", error))?;
        Ok(values.bits & 1 != 0)
    }
}

fn verify_input(flags: u64) -> Result<(), String> {
    if flags & (INPUT | OUTPUT) != INPUT {
        return Err("GPIO line must already be an input".into());
    }
    Ok(())
}

fn sample_input(
    chip: &impl Chip,
    offset: u32,
    samples: u64,
    deadline: Deadline,
) -> Result<(Duration, u64), String> {
    deadline.check()?;
    let initial = chip.flags(offset)?;
    if initial & USED != 0 {
        return Err("GPIO line is busy or reserved".into());
    }
    verify_input(initial)?;
    let polarity = initial & ACTIVE_LOW;
    deadline.check()?;
    let line = chip.request_as_is(offset, polarity)?;
    // Check direction again under the line request before the first read.
    deadline.check()?;
    let acquired = chip.flags(offset)?;
    verify_input(acquired)?;
    if acquired & ACTIVE_LOW != polarity {
        return Err("GPIO line polarity changed during request".into());
    }
    let mut active = 0;
    let elapsed = peripheral::measure_reads(samples, deadline, || {
        active += u64::from(chip.active(&line)?);
        Ok(())
    })?;
    Ok((elapsed, active))
}

pub(crate) fn run(
    chip: &Path,
    offset: u32,
    samples: u64,
    deadline: Deadline,
) -> Result<(), String> {
    deadline.check()?;
    let device = peripheral::open_device(chip, Kind::Gpio, false)?;
    let mut info = ChipInfo::default();
    // GPIO_GET_CHIPINFO_IOCTL is shared by the v1 and v2 APIs.
    unsafe { peripheral::ioctl_ptr(&device, GET_CHIP_INFO, &mut info) }
        .map_err(|error| format!("cannot query GPIO chip: {error}"))?;
    if offset >= info.lines {
        return Err(format!(
            "GPIO line offset {offset} is out of range for a {}-line chip",
            info.lines
        ));
    }
    let (elapsed, active) = sample_input(&device, offset, samples, deadline)?;
    crate::report::metric(
        "Mean GPIO read latency",
        elapsed.as_secs_f64() * 1_000_000.0 / samples as f64,
        "us",
    );
    report_line!("GPIO read latency (userspace chardev v2 ioctl)");
    report_line!("Chip: {}", chip.display());
    report_line!("Line offset: {offset}");
    report_line!("Samples: {samples}");
    report_line!("Active samples: {active}");
    report_line!("Measured elapsed: {:.6} s", elapsed.as_secs_f64());
    report_line!(
        "Mean read latency: {:.3} us",
        elapsed.as_secs_f64() * 1_000_000.0 / samples as f64
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct Fixture {
        initial: u64,
        acquired: u64,
        requests: Cell<u64>,
        reads: Cell<u64>,
        released: Cell<bool>,
    }

    struct Line<'a>(&'a Cell<bool>);

    impl Drop for Line<'_> {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    impl<'a> Chip for &'a Fixture {
        type Line = Line<'a>;

        fn flags(&self, offset: u32) -> Result<u64, String> {
            assert_eq!(offset, 3);
            Ok(if self.requests.get() == 0 {
                self.initial
            } else {
                self.acquired
            })
        }

        fn request_as_is(&self, offset: u32, flags: u64) -> Result<Self::Line, String> {
            assert_eq!(offset, 3);
            assert_eq!(flags, self.initial & ACTIVE_LOW);
            self.requests.set(self.requests.get() + 1);
            Ok(Line(&self.released))
        }

        fn active(&self, _line: &Self::Line) -> Result<bool, String> {
            let count = self.reads.get();
            self.reads.set(count + 1);
            Ok(count % 2 == 0)
        }
    }

    fn fixture(initial: u64, acquired: u64) -> Fixture {
        Fixture {
            initial,
            acquired,
            requests: Cell::new(0),
            reads: Cell::new(0),
            released: Cell::new(false),
        }
    }

    #[test]
    fn as_is_input_request_checks_direction_busy_and_read_count() {
        let deadline = || Deadline::after(Duration::from_secs(5));
        let input = fixture(INPUT | ACTIVE_LOW, INPUT | ACTIVE_LOW | USED);
        assert_eq!(sample_input(&&input, 3, 11, deadline()).unwrap().1, 6);
        assert_eq!(input.requests.get(), 1);
        assert_eq!(input.reads.get(), 11);
        assert!(input.released.get());

        for (flags, error) in [(OUTPUT, "input"), (INPUT | USED, "busy"), (0, "input")] {
            let invalid = fixture(flags, INPUT);
            assert!(sample_input(&&invalid, 3, 1, deadline())
                .unwrap_err()
                .contains(error));
            assert_eq!(invalid.requests.get(), 0);
            assert_eq!(invalid.reads.get(), 0);
        }

        let race = fixture(INPUT, OUTPUT | USED);
        assert!(sample_input(&&race, 3, 1, deadline())
            .unwrap_err()
            .contains("input"));
        assert_eq!(race.requests.get(), 1);
        assert_eq!(race.reads.get(), 0);
        assert!(race.released.get());
    }

    #[test]
    fn gpio_abi_layout_requests_and_errors_match_linux() {
        // Both asm-generic and MIPS encode a combined read/write as 0xc0000000.
        assert_eq!(
            (GET_INFO, GET_LINE, GET_VALUES),
            (0xc100b405, 0xc250b407, 0xc010b40e)
        );
        #[cfg(not(target_arch = "mips"))]
        assert_eq!(GET_CHIP_INFO, 0x8044b401);
        #[cfg(target_arch = "mips")]
        assert_eq!(GET_CHIP_INFO, 0x4044b401);
        let request = Request::input_as_is(3, INPUT | ACTIVE_LOW | (1 << 8));
        let base = &request as *const Request as usize;
        assert_eq!(&request.config as *const Config as usize - base, 288);
        assert_eq!(&request.fd as *const i32 as usize - base, 588);
        assert_eq!(request.offsets[0], 3);
        assert_eq!(request.num_lines, 1);
        assert_eq!(request.config.flags, ACTIVE_LOW);
        assert_eq!(request.config.num_attrs, 0);
        assert!(operation_error("test", io::Error::from_raw_os_error(25)).contains("unavailable"));
        assert!(operation_error("test", io::Error::from_raw_os_error(16)).contains("busy"));
    }
}

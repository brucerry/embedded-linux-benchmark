use crate::common::Deadline;
use crate::peripheral::{self, Kind};
use std::io::Read;
use std::os::raw::c_ulong;
use std::path::Path;
use std::time::Duration;

pub(crate) const MAX_READ_BYTES: u64 = 32;
pub(crate) const DEFAULT_SAMPLES: u64 = 32;
pub(crate) const MAX_SAMPLES: u64 = 1024;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;

const I2C_FUNCS: u32 = 0x0705;
const I2C_FUNC_I2C: c_ulong = 0x00000001;

pub(crate) fn address(raw: &str) -> Result<u8, String> {
    let addr = match raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16),
        None => raw.parse::<u8>(),
    };
    addr.ok()
        .filter(|addr| (0x08..=0x77).contains(addr))
        .ok_or_else(|| {
            "'--address' requires an unreserved 7-bit address (0x08 through 0x77)".into()
        })
}

fn sample_reads(
    mut adapter: impl Read,
    bytes: usize,
    samples: u64,
    deadline: Deadline,
) -> Result<Duration, String> {
    let mut buffer = [0; MAX_READ_BYTES as usize];
    peripheral::measure_reads(samples, deadline, || {
        // One read call is one pure I2C transaction. Never retry or fill a short read.
        let received = adapter
            .read(&mut buffer[..bytes])
            .map_err(|error| format!("I2C fixture read failed: {error}"))?;
        if received != bytes {
            return Err(format!("I2C fixture returned {received} of {bytes} bytes"));
        }
        Ok(())
    })
}

pub(crate) fn run(
    adapter: &Path,
    address: u8,
    bytes: usize,
    samples: u64,
    deadline: Deadline,
) -> Result<(), String> {
    deadline.check()?;
    let file = peripheral::open_device(adapter, Kind::I2c, false)?;
    let mut functions: c_ulong = 0;
    // I2C_FUNCS writes an unsigned long capability mask.
    unsafe { peripheral::ioctl_ptr(&file, I2C_FUNCS, &mut functions) }
        .map_err(|error| format!("cannot query I2C adapter capabilities: {error}"))?;
    if functions & I2C_FUNC_I2C == 0 {
        return Err("I2C adapter does not support raw I2C reads".into());
    }
    deadline.check()?;
    peripheral::ioctl_i2c_slave(&file, address).map_err(|error| {
        if error.raw_os_error() == Some(peripheral::EBUSY) {
            format!("I2C address 0x{address:02x} is busy: {error}")
        } else {
            format!("cannot select I2C address 0x{address:02x}: {error}")
        }
    })?;
    let measured = sample_reads(file, bytes, samples, deadline)?;
    crate::report::metric(
        "Mean I2C read latency",
        measured.as_secs_f64() * 1_000_000.0 / samples as f64,
        "us",
    );
    report_line!("I2C read latency (userspace pure read)");
    report_line!("Adapter: {}", adapter.display());
    report_line!("Address: 0x{address:02x}");
    report_line!("Samples: {samples}");
    report_line!("Bytes received: {}", samples * bytes as u64);
    report_line!("Measured elapsed: {:.6} s", measured.as_secs_f64());
    report_line!(
        "Mean read latency: {:.3} us",
        measured.as_secs_f64() * 1_000_000.0 / samples as f64
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_sample_is_one_fixed_read_and_short_or_expired_reads_stop() {
        let deadline = || Deadline::after(Duration::from_secs(5));
        let all = &b"abcdefgh"[..];
        let mut data = all;
        sample_reads(&mut data, 3, 2, deadline()).unwrap();
        assert_eq!(data, b"gh");
        assert!(sample_reads(&mut data, 3, 3, deadline())
            .unwrap_err()
            .contains("2 of 3"));
        let mut data = all;
        assert!(
            sample_reads(&mut data, 3, 1, Deadline::after(Duration::ZERO))
                .unwrap_err()
                .contains("time limit")
        );
        assert_eq!(data, all);
    }

    #[test]
    fn address_parser_excludes_reserved_ranges() {
        assert_eq!(address("0x08"), Ok(8));
        assert_eq!(address("119"), Ok(119));
        for invalid in ["0x07", "0x78", "128", "-1", "", "0x300", "0xz"] {
            assert!(address(invalid).is_err());
        }
    }
}

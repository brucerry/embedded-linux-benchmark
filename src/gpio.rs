use crate::common::Deadline;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_SAMPLES: u64 = 1_000;
pub(crate) const MAX_SAMPLES: u64 = 100_000;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;

fn open_attribute(path: &Path) -> Result<File, String> {
    let error =
        |error: std::io::Error| format!("cannot read GPIO attribute {}: {error}", path.display());
    let wrong_type = || format!("GPIO attribute {} must be a regular file", path.display());
    if !fs::symlink_metadata(path).map_err(error)?.is_file() {
        return Err(wrong_type());
    }
    let file = File::open(path).map_err(error)?;
    if !file.metadata().map_err(error)?.is_file() {
        return Err(wrong_type());
    }
    Ok(file)
}

fn input_benchmark(
    gpio: &Path,
    samples: u64,
    deadline: Deadline,
) -> Result<(Duration, u64), String> {
    deadline.check()?;
    let mut direction = [0; 16];
    let count = open_attribute(&gpio.join("direction"))?
        .read(&mut direction)
        .map_err(|error| format!("cannot read GPIO direction: {error}"))?;
    if !matches!(&direction[..count], b"in" | b"in\n") {
        return Err("GPIO must already be exported and configured as an input".into());
    }
    let mut value = open_attribute(&gpio.join("value"))?;
    let mut high = 0;
    let mut measured = Duration::ZERO;
    for _ in 0..samples {
        deadline.check()?;
        let start = Instant::now();
        value
            .seek(SeekFrom::Start(0))
            .map_err(|error| format!("cannot rewind GPIO value: {error}"))?;
        let mut byte = [0];
        // A single-byte read also prevents an unbounded retry on interruption.
        let count = value
            .read(&mut byte)
            .map_err(|error| format!("cannot read GPIO value: {error}"))?;
        measured += start.elapsed();
        match (count, byte[0]) {
            (1, b'0') => (),
            (1, b'1') => high += 1,
            _ => return Err("GPIO value must start with 0 or 1".into()),
        }
    }
    deadline.check()?;
    Ok((measured, high))
}

pub(crate) fn run(pin: u32, samples: u64, deadline: Deadline) -> Result<(), String> {
    let path = Path::new("/sys/class/gpio").join(format!("gpio{pin}"));
    let (measured, high) = input_benchmark(&path, samples, deadline)?;
    println!("GPIO read latency (userspace sysfs seek + read)");
    println!("GPIO: {pin}");
    println!("Samples: {samples}");
    println!("High samples: {high}");
    println!("Measured elapsed: {:.6} s", measured.as_secs_f64());
    println!(
        "Mean read latency: {:.3} us",
        measured.as_secs_f64() * 1_000_000.0 / samples as f64
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn new() -> Self {
            let time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir =
                std::env::temp_dir().join(format!("embedded-gpio-{}-{time}", std::process::id()));
            fs::create_dir(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn reads_only_an_existing_input_and_checks_value_type() {
        let fixture = Fixture::new();
        let deadline = || Deadline::after(Duration::from_secs(5));
        let direction = fixture.0.join("direction");
        let value = fixture.0.join("value");

        assert!(input_benchmark(&fixture.0, 1, deadline()).is_err());
        fs::write(&direction, "out\n").unwrap();
        assert!(input_benchmark(&fixture.0, 1, deadline())
            .unwrap_err()
            .contains("input"));
        fs::write(&direction, "in\n").unwrap();
        fs::write(&value, "1\n").unwrap();
        assert_eq!(input_benchmark(&fixture.0, 17, deadline()).unwrap().1, 17);
        assert_eq!(fs::read_to_string(&direction).unwrap(), "in\n");
        assert_eq!(fs::read_to_string(&value).unwrap(), "1\n");
        assert!(
            input_benchmark(&fixture.0, 1, Deadline::after(Duration::ZERO))
                .unwrap_err()
                .contains("time limit")
        );
        fs::write(&value, "0\n").unwrap();
        assert_eq!(input_benchmark(&fixture.0, 3, deadline()).unwrap().1, 0);
        fs::write(&value, "invalid\n").unwrap();
        assert!(input_benchmark(&fixture.0, 1, deadline())
            .unwrap_err()
            .contains("0 or 1"));
        fs::remove_file(&value).unwrap();
        symlink(&direction, &value).unwrap();
        assert!(input_benchmark(&fixture.0, 1, deadline())
            .unwrap_err()
            .contains("regular file"));
    }
}

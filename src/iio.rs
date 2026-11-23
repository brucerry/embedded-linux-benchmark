use crate::common::Deadline;
use crate::sysfs::{name_digits, open_attribute};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Instant;

pub(crate) const DEFAULT_SAMPLES: u64 = 100;
pub(crate) const MAX_SAMPLES: u64 = 10_000;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;

fn channel_path(root: &Path, device: &str, channel: &str) -> Result<std::path::PathBuf, String> {
    if !device.strip_prefix("iio:device").map_or(false, |suffix| {
        name_digits(suffix) && suffix.parse::<u32>().is_ok()
    }) {
        return Err("expected an explicit IIO device name like 'iio:device0'".into());
    }
    let name = channel.strip_prefix("in_").unwrap_or("");
    if name.is_empty()
        || !name.as_bytes()[0].is_ascii_lowercase()
        || channel.len() > 64
        || channel.ends_with("_raw")
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
        })
    {
        return Err(
            "expected an IIO input channel stem like 'in_voltage0' without the '_raw' suffix"
                .into(),
        );
    }
    Ok(root.join(device).join(format!("{channel}_raw")))
}

fn read_raw(file: &mut File) -> Result<i64, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("cannot rewind IIO attribute: {error}"))?;
    let mut buffer = [0u8; 65];
    let count = file
        .read(&mut buffer)
        .map_err(|error| format!("cannot read IIO raw sample: {error}"))?;
    if count == buffer.len() || !buffer[..count].ends_with(b"\n") {
        return Err("invalid or incomplete IIO raw sample".into());
    }
    std::str::from_utf8(&buffer[..count])
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .ok_or_else(|| "IIO raw sample is not a signed integer".into())
}

struct Stats {
    count: u64,
    raw_min: i64,
    raw_max: i64,
    raw_sum: i128,
    interval_mean: f64,
    interval_m2: f64,
    read_seconds: f64,
    last: Option<Instant>,
}

impl Stats {
    fn record(&mut self, value: i64, start: Instant, stop: Instant) {
        self.raw_min = self.raw_min.min(value);
        self.raw_max = self.raw_max.max(value);
        self.raw_sum += i128::from(value);
        self.read_seconds += stop.duration_since(start).as_secs_f64();
        if let Some(last) = self.last {
            let interval = stop.duration_since(last).as_secs_f64();
            let delta = interval - self.interval_mean;
            self.interval_mean += delta / self.count as f64;
            self.interval_m2 += delta * (interval - self.interval_mean);
        }
        self.count += 1;
        self.last = Some(stop);
    }
}

fn collect(
    count: u64,
    deadline: Deadline,
    mut read: impl FnMut() -> Result<i64, String>,
) -> Result<Stats, String> {
    let mut stats = Stats {
        count: 0,
        raw_min: i64::MAX,
        raw_max: i64::MIN,
        raw_sum: 0,
        interval_mean: 0.0,
        interval_m2: 0.0,
        read_seconds: 0.0,
        last: None,
    };
    for _ in 0..count {
        deadline.check()?;
        let start = Instant::now();
        let sample = read()?;
        let stop = Instant::now();
        deadline.check()?;
        stats.record(sample, start, stop);
    }
    Ok(stats)
}

pub(crate) fn run(
    device: &str,
    channel: &str,
    samples: u64,
    deadline: Deadline,
) -> Result<(), String> {
    let path = channel_path(Path::new("/sys/bus/iio/devices"), device, channel)?;
    deadline.check()?;
    let mut file = open_attribute(&path)
        .map_err(|error| format!("cannot open IIO input {}: {error}", path.display()))?;
    let result = collect(samples, deadline, || read_raw(&mut file))?;
    println!("IIO raw sampling (read-only userspace sysfs)");
    println!("Device: {device}");
    println!("Channel: {channel}_raw");
    println!("Samples: {}", result.count);
    println!(
        "Raw code min / max: {} / {}",
        result.raw_min, result.raw_max
    );
    println!(
        "Raw code mean: {:.3}",
        result.raw_sum as f64 / result.count as f64
    );
    println!(
        "Mean seek + read time: {:.3} us",
        result.read_seconds * 1_000_000.0 / result.count as f64
    );
    if result.interval_mean > 0.0 {
        println!(
            "Userspace cadence: {:.2} samples/s",
            1.0 / result.interval_mean
        );
    } else {
        println!("Userspace cadence: below timer resolution");
    }
    let jitter = (result.interval_m2 / (result.count - 1) as f64)
        .max(0.0)
        .sqrt();
    println!(
        "Userspace interval jitter (standard deviation): {:.3} us",
        jitter * 1_000_000.0
    );
    println!("Hardware timestamps: unavailable in raw sysfs sampling");
    println!("Calibration: raw codes");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn raw_input_path_and_samples_stay_within_the_selected_read_only_channel() {
        let root = std::env::temp_dir().join(format!("benchmark-iio-{}", std::process::id()));
        fs::create_dir_all(root.join("iio:device0")).unwrap();
        let path = channel_path(&root, "iio:device0", "in_voltage0-voltage1").unwrap();
        fs::write(&path, "-123\n").unwrap();
        let mut handle = open_attribute(&path).unwrap();
        let result = collect(16, Deadline::after(Duration::from_secs(2)), || {
            read_raw(&mut handle)
        })
        .unwrap();
        assert_eq!(
            (result.count, result.raw_min, result.raw_max, result.raw_sum),
            (16, -123, -123, -1968)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "-123\n");
        fs::write(&path, "3.5\n").unwrap();
        assert!(read_raw(&mut handle)
            .unwrap_err()
            .contains("signed integer"));
        fs::write(&path, "5").unwrap();
        assert!(read_raw(&mut handle).unwrap_err().contains("incomplete"));
        fs::remove_dir_all(root).unwrap();
        assert!(collect(2, Deadline::after(Duration::ZERO), || panic!(
            "expired sample was read"
        ))
        .is_err());
        assert!(channel_path(Path::new("/nowhere"), "iio:device0", "in_voltage0_raw").is_err());
        assert!(channel_path(Path::new("/nowhere"), "../iio:device0", "in_temp").is_err());
    }
}

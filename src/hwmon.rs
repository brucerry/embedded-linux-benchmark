//! Read only the explicitly selected hwmon temperature attribute.
use crate::pcie_health::json_string;
use crate::sysfs::{attribute_status, bounded_text};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

fn valid_sensor(sensor: &str) -> bool {
    let Some(n) = sensor.strip_prefix("hwmon") else {
        return false;
    };
    !n.is_empty()
        && n.len() <= 7
        && (n == "0" || !n.starts_with('0'))
        && n.bytes().all(|b| b.is_ascii_digit())
}

fn report(index: &Path, devices: &Path, sensor: &str, channel: u64) -> Result<String, String> {
    if !valid_sensor(sensor) || !(1..=32).contains(&channel) {
        return Err("expected an explicit hwmon index and temperature channel 1–32".to_owned());
    }
    let entry = index.join(sensor);
    let identify = || {
        let path = fs::canonicalize(&entry).map_err(attribute_status)?;
        if !path.starts_with(devices) || path.file_name().and_then(|s| s.to_str()) != Some(sensor) {
            return Err("invalid");
        }
        let stat = fs::symlink_metadata(&path).map_err(attribute_status)?;
        if !stat.is_dir() {
            return Err("invalid");
        }
        Ok((path, stat.dev(), stat.ino()))
    };
    let sensor = json_string(sensor);
    let base = |status| {
        format!(
            r#"{{"schema_version":2,"kind":"hwmon_temperature","sensor":{sensor},"channel":{channel},"status":"{status}"}}"#
        )
    };
    let initial = match identify() {
        Ok(value) => value,
        Err(status) => return Ok(base(status)),
    };
    let value = bounded_text(&initial.0.join(format!("temp{channel}_input"))).and_then(|raw| {
        let v = raw.parse::<i64>().map_err(|_| "invalid")?;
        if raw != v.to_string() {
            return Err("invalid");
        }
        Ok(v)
    });
    if identify().ok().as_ref() != Some(&initial) {
        return Ok(base("changed"));
    }
    match value {
        Ok(v) => Ok(format!(
            r#"{{"schema_version":2,"kind":"hwmon_temperature","sensor":{sensor},"channel":{channel},"status":"observed","raw_value":"{v}","unit":"unknown"}}"#
        )),
        Err(status) => Ok(base(status)),
    }
}

pub(crate) fn run(sensor: &str, channel: u64) -> Result<(), String> {
    println!(
        "{}",
        report(
            Path::new("/sys/class/hwmon"),
            Path::new("/sys/devices"),
            sensor,
            channel
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn selected_channel_keeps_zero_and_refuses_aliases_and_invalid_values() {
        let root = std::env::temp_dir().join(format!("eld-hwmon-temp-{}", std::process::id()));
        let devices = root.join("devices");
        let class = root.join("class");
        let sensor = devices.join("platform/example/power_supply/sensor/hwmon0");
        fs::create_dir_all(&sensor).unwrap();
        fs::create_dir_all(&class).unwrap();
        symlink(&sensor, class.join("hwmon0")).unwrap();
        fs::write(sensor.join("temp1_input"), "0\n").unwrap();
        fs::write(sensor.join("temp2_input"), "950\n").unwrap();
        fs::write(sensor.join("temp5_input"), "bad\n").unwrap();
        symlink(sensor.join("temp1_input"), sensor.join("temp3_input")).unwrap();
        let check = |channel| report(&class, &devices, "hwmon0", channel).unwrap();
        assert!(check(1).contains(r#""status":"observed","raw_value":"0","unit":"unknown""#));
        assert!(check(2).contains(r#""status":"observed","raw_value":"950","unit":"unknown""#));
        assert!(!check(2).contains("millicelsius"));
        assert!(check(3).contains(r#""status":"invalid""#));
        assert!(check(4).contains(r#""status":"unavailable""#));
        assert!(check(5).contains(r#""status":"invalid""#));
        assert!(report(&class, &devices, "../hwmon0", 1).is_err());
        assert!(report(&class, &devices, "hwmon00", 1).is_err());
        assert!(report(&class, &devices, "hwmon0", 0).is_err());
        let outside = root.join("outside/hwmon1");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("temp1_input"), "42\n").unwrap();
        symlink(&outside, class.join("hwmon1")).unwrap();
        let withheld = report(&class, &devices, "hwmon1", 1).unwrap();
        assert!(withheld.contains(r#""status":"invalid""#));
        assert!(!withheld.contains("raw_value"));
        fs::remove_dir_all(root).unwrap();
    }
}

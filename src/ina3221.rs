//! Explicit, read-only INA3221 hwmon adapter. Not an I2C bus scan.
use crate::{pcie_health::json_string, sysfs};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

fn node_name(node: &str) -> Result<&str, String> {
    let valid = node.split_once('-').is_some_and(|(bus, address)| {
        sysfs::name_digits(bus)
            && bus.len() <= 5
            && (bus == "0" || !bus.starts_with('0'))
            && ["0040", "0041", "0042", "0043"].contains(&address)
    });
    valid
        .then_some(node)
        .ok_or_else(|| "expected an INA3221 device ID like 1-0040".into())
}

fn key(path: &Path) -> Result<(u64, u64), &'static str> {
    let meta = fs::symlink_metadata(path).map_err(sysfs::attribute_status)?;
    if !meta.is_dir() {
        return Err("invalid");
    }
    Ok((meta.dev(), meta.ino()))
}

fn device_at(path: &Path, node: &str, devices: &Path) -> Result<(PathBuf, u64, u64), &'static str> {
    if !fs::symlink_metadata(path)
        .map_err(sysfs::attribute_status)?
        .is_symlink()
    {
        return Err("invalid");
    }
    let target = fs::canonicalize(path).map_err(sysfs::attribute_status)?;
    if !target.starts_with(devices) || target.file_name().and_then(|x| x.to_str()) != Some(node) {
        return Err("invalid");
    }
    let (dev, ino) = key(&target)?;
    Ok((target, dev, ino))
}

fn label(path: &Path) -> String {
    match sysfs::bounded_text(path).and_then(|s| {
        if s.is_empty() || s.len() > 64 || s.chars().any(char::is_control) {
            Err("invalid")
        } else {
            Ok(s)
        }
    }) {
        Ok(text) => format!(r#"{{"status":"observed","value":{}}}"#, json_string(&text)),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

fn scan(node: &str, driver: &Path, devices: &Path) -> Result<Vec<String>, &'static str> {
    let binding = key(driver)?;
    let node_link = driver.join(node);
    let device = device_at(&node_link, node, devices)?;
    let reverse = device.0.join("driver");
    let bound = || {
        fs::symlink_metadata(&reverse)
            .map_err(sysfs::attribute_status)?
            .is_symlink()
            .then_some(())
            .ok_or("invalid")
            .and_then(|_| {
                (fs::canonicalize(&reverse).map_err(sysfs::attribute_status)? == driver)
                    .then_some(())
                    .ok_or("invalid")
            })
    };
    bound()?;
    let hwmon = device.0.join("hwmon");
    let root_identity = key(&hwmon)?;
    let mut entries = fs::read_dir(&hwmon).map_err(sysfs::attribute_status)?;
    let mut rows = Vec::new();
    for entry in entries.by_ref().take(32) {
        let entry = entry.map_err(sysfs::attribute_status)?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name
            .strip_prefix("hwmon")
            .is_some_and(|v| v.len() <= 10 && sysfs::name_digits(v))
        {
            continue;
        }
        let path = entry.path();
        let identity = key(&path)?;
        if sysfs::bounded_text(&path.join("name"))? != "ina3221" {
            continue;
        }
        let sensors = (1..=3).map(|n| {
            let rail = label(&path.join(format!("in{n}_label")));
            let voltage = sysfs::numeric_json(&path.join(format!("in{n}_input")),true);
            let current = sysfs::numeric_json(&path.join(format!("curr{n}_input")),true);
            format!(r#"{{"channel":{n},"label":{rail},"voltage_millivolts":{voltage},"current_milliamps":{current}}}"#)
        }).collect::<Vec<_>>().join(",");
        if key(&path)? != identity || sysfs::bounded_text(&path.join("name"))? != "ina3221" {
            return Err("changed");
        }
        rows.push(format!(r#"{{"sensor":"{name}","channels":[{sensors}]}}"#));
    }
    if entries.next().is_some() {
        return Err("scan_capped");
    }
    if key(driver)? != binding
        || key(&hwmon)? != root_identity
        || bound().is_err()
        || device_at(&node_link, node, devices)? != device
    {
        return Err("changed");
    }
    rows.sort_unstable();
    if rows.is_empty() {
        return Err("unavailable");
    }
    Ok(rows)
}

fn render(node: &str, driver: &Path, devices: &Path) -> String {
    let (status, rows) = match scan(node, driver, devices) {
        Ok(v) => ("observed", v.join(",")),
        Err(status) => (status, String::new()),
    };
    format!(
        r#"{{"schema_version":1,"kind":"ina3221_rail_observation","i2c_device":"{node}","status":"{status}","sampling":"sequential_sysfs_reads","sensors":[{rows}]}}"#
    )
}

pub(crate) fn run(node: &str) -> Result<(), String> {
    let node = node_name(node)?;
    println!(
        "{}",
        render(
            node,
            Path::new("/sys/bus/i2c/drivers/ina3221"),
            Path::new("/sys/devices")
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn same_address_keeps_the_board_rail_label_without_inferring_totals() {
        let root = std::env::temp_dir().join(format!("eld-ina3221-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _guard = Cleanup(root.clone());
        let devices = root.join("devices");
        let driver = root.join("drivers/ina3221");
        let dev = devices.join("i2c-1/1-0040");
        let sensor = dev.join("hwmon/hwmon8");
        for p in [&sensor, &driver] {
            fs::create_dir_all(p).unwrap();
        }
        symlink(&dev, driver.join("1-0040")).unwrap();
        symlink(&driver, dev.join("driver")).unwrap();
        fs::write(sensor.join("name"), "ina3221\n").unwrap();
        fs::write(sensor.join("in1_input"), "5050\n").unwrap();
        fs::write(sensor.join("curr1_input"), "-31\n").unwrap();
        fs::write(sensor.join("curr2_input"), "+123\n").unwrap();
        fs::write(sensor.join("in3_label"), "unsafe\nextra\n").unwrap();
        fs::write(sensor.join("curr4_input"), "1234567\n").unwrap();
        for (reported, other) in [("VDD_IN", "VDD_GPU_SOC"), ("VDD_GPU_SOC", "VDD_IN")] {
            fs::write(sensor.join("in1_label"), format!("{reported}\n")).unwrap();
            let out = render("1-0040", &driver, &devices);
            assert!(out.contains(r#""status":"observed","sampling":"sequential_sysfs_reads""#));
            assert!(out.contains(&format!(
                r#""label":{{"status":"observed","value":"{reported}"}}"#
            )));
            assert!(!out.contains(other));
            assert!(out.contains(r#""voltage_millivolts":{"status":"observed","value":"5050"}"#));
            assert!(out.contains(r#""current_milliamps":{"status":"observed","value":"-31"}"#));
            assert!(out.contains(r#""channel":2,"label":{"status":"unavailable"}"#));
            assert!(out.contains(r#""current_milliamps":{"status":"invalid"}"#));
            assert!(out.contains(r#""channel":3,"label":{"status":"invalid"}"#));
            assert!(!out.contains(r#""channel":4"#));
            assert!(!out.contains("1234567"));
        }
        let outside = root.join("1-0041");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, driver.join("1-0041")).unwrap();
        assert!(render("1-0041", &driver, &devices).contains(r#""status":"invalid""#));
        assert!(render("1-0042", &driver, &devices).contains(r#""status":"unavailable""#));
        for bad in ["../1-0040", "01-0040", "1-0044", "1-40", "1-0040/"] {
            assert!(node_name(bad).is_err());
        }
    }
}

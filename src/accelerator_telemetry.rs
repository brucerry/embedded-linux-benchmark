//! Explicit AMDGPU hwmon telemetry; never writes controls or opens PCI resources.
use crate::{inventory, pcie, sysfs};
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const ROOT_LIMIT: usize = 64;

fn error(e: io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    }
}

fn key(path: &Path) -> Result<(u64, u64), &'static str> {
    let stat = fs::symlink_metadata(path).map_err(error)?;
    if !stat.is_dir() {
        return Err("invalid");
    }
    Ok((stat.dev(), stat.ino()))
}

fn read(path: &Path) -> Result<String, &'static str> {
    let mut raw = String::new();
    sysfs::open_attribute(path)
        .map_err(error)?
        .take(129)
        .read_to_string(&mut raw)
        .map_err(error)?;
    if raw.len() > 128 {
        return Err("invalid");
    }
    Ok(raw.trim().to_owned())
}

fn numeric(path: &Path, signed: bool) -> String {
    let value = read(path).and_then(|raw| {
        let digits = if signed {
            raw.strip_prefix('-').unwrap_or(&raw)
        } else {
            &raw
        };
        if !sysfs::name_digits(digits) {
            return Err("invalid");
        }
        if signed {
            raw.parse::<i64>()
                .map(|v| v.to_string())
                .map_err(|_| "invalid")
        } else {
            raw.parse::<u64>()
                .map(|v| v.to_string())
                .map_err(|_| "invalid")
        }
    });
    match value {
        Ok(value) => format!(r#"{{"status":"observed","value":"{value}"}}"#),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

fn amdgpu_binding(target: &Path, drivers: &Path) -> Result<(u64, u64), &'static str> {
    let link = target.join("driver");
    let expected = drivers.join("amdgpu");
    match fs::symlink_metadata(&link) {
        Ok(m) if m.is_symlink() => {}
        Ok(_) => return Err("invalid"),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err("unsupported"),
        Err(e) => return Err(error(e)),
    }
    if fs::canonicalize(&link).map_err(error)? != expected {
        return Err("unsupported");
    }
    key(&expected)
}

fn scan(
    bdf: &str,
    index: &Path,
    devices: &Path,
    drivers: &Path,
) -> Result<Vec<String>, &'static str> {
    let device = inventory::pci_entry(index, devices, bdf)?.ok_or("unavailable")?;
    if inventory::pci_attr(&device.0.join("vendor"))? != 0x1002 {
        return Err("unsupported");
    }
    let driver = amdgpu_binding(&device.0, drivers)?;
    let root = device.0.join("hwmon");
    let initial = key(&root)?;
    let mut listing = fs::read_dir(&root).map_err(error)?;
    let mut rows = Vec::new();
    for item in listing.by_ref().take(ROOT_LIMIT) {
        let item = item.map_err(error)?;
        let name = item.file_name();
        let Some(name) = name.to_str() else { continue };
        if !name
            .strip_prefix("hwmon")
            .is_some_and(|s| s.len() <= 10 && sysfs::name_digits(s))
        {
            continue;
        }
        let path = item.path();
        let before = key(&path)?;
        if read(&path.join("name"))? != "amdgpu" {
            continue;
        }
        let temperatures = (1..=3)
            .map(|i| numeric(&path.join(format!("temp{i}_input")), true))
            .collect::<Vec<_>>()
            .join(",");
        let power_average = numeric(&path.join("power1_average"), false);
        let power_input = numeric(&path.join("power1_input"), false);
        if key(&path)? != before || read(&path.join("name"))? != "amdgpu" {
            return Err("changed");
        }
        rows.push(format!(
            r#"{{"sensor":"{name}","temperature_on_die_millicelsius_channels_1_to_3":[{temperatures}],"soc_power_average_microwatts":{power_average},"soc_power_instant_microwatts":{power_input}}}"#
        ));
    }
    if listing.next().is_some() {
        return Err("scan_capped");
    }
    if key(&root)? != initial
        || amdgpu_binding(&device.0, drivers) != Ok(driver)
        || inventory::pci_entry(index, devices, bdf)? != Some(device)
    {
        return Err("changed");
    }
    rows.sort_unstable();
    if rows.is_empty() {
        return Err("unavailable");
    }
    Ok(rows)
}

fn render(bdf: &str, index: &Path, devices: &Path, drivers: &Path) -> String {
    let (status, sensors) = match scan(bdf, index, devices, drivers) {
        Ok(rows) => ("observed", rows.join(",")),
        Err(status) => (status, String::new()),
    };
    format!(
        r#"{{"schema_version":1,"kind":"amdgpu_hwmon_observation","bdf":"{bdf}","status":"{status}","runtime_probe":"not_performed","sensors":[{sensors}]}}"#
    )
}

pub(crate) fn run(device: &str) -> Result<(), String> {
    let bdf = pcie::bdf(device)?;
    println!(
        "{}",
        render(
            &bdf,
            Path::new("/sys/bus/pci/devices"),
            Path::new("/sys/devices"),
            Path::new("/sys/bus/pci/drivers"),
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn amdgpu_read_is_scoped_and_preserves_signed_and_wide_values() {
        let dir = std::env::temp_dir().join(format!("eld-hwmon-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _guard = Cleanup(dir.clone());
        let index = dir.join("index");
        let drivers = dir.join("drivers");
        let correct = drivers.join("amdgpu");
        let devices = dir.join("devices");
        let bdf = "0000:01:00.0";
        let dev = devices.join(bdf);
        let sensor = dev.join("hwmon/hwmon4");
        for s in [&index, &correct, &sensor] {
            fs::create_dir_all(s).unwrap();
        }
        symlink(&dev, index.join(bdf)).unwrap();
        fs::write(dev.join("vendor"), "0x1002\n").unwrap();
        fs::write(sensor.join("name"), "amdgpu\n").unwrap();
        let driver_link = dev.join("driver");
        symlink(&correct, &driver_link).unwrap();
        fs::write(sensor.join("temp1_input"), "-2000\n").unwrap();
        fs::write(sensor.join("temp3_input"), "+2000\n").unwrap();
        fs::write(sensor.join("power1_average"), "9007199254740993\n").unwrap();
        fs::write(sensor.join("power1_input"), "-10\n").unwrap();
        let sample = render(bdf, &index, &devices, &drivers);
        assert!(sample.contains(r#""status":"observed","runtime_probe":"not_performed""#));
        assert!(sample.contains(r#"[{"status":"observed","value":"-2000"},{"status":"unavailable"},{"status":"invalid"}]"#));
        assert!(sample.contains(
            r#""soc_power_average_microwatts":{"status":"observed","value":"9007199254740993"}"#
        ));
        assert!(sample.contains(r#""soc_power_instant_microwatts":{"status":"invalid"}"#));
        fs::remove_file(&driver_link).unwrap();
        symlink(&sensor, &driver_link).unwrap();
        assert!(render(bdf, &index, &devices, &drivers).contains(r#""status":"unsupported""#));
        assert!(render("0000:09:00.0", &index, &devices, &drivers)
            .contains(r#""status":"unavailable""#));
    }
}

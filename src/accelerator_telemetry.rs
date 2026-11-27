//! Explicit AMDGPU hwmon and memory RAS telemetry; never writes controls or opens PCI resources.
use crate::{inventory, pcie, sysfs};
use std::fs;
use std::io;
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
            .map_or(false, |s| s.len() <= 10 && sysfs::name_digits(s))
        {
            continue;
        }
        let path = item.path();
        let before = key(&path)?;
        if sysfs::bounded_text(&path.join("name"))? != "amdgpu" {
            continue;
        }
        let temperatures = (1..=3)
            .map(|i| sysfs::numeric_json(&path.join(format!("temp{i}_input")), true))
            .collect::<Vec<_>>()
            .join(",");
        let power_average = sysfs::numeric_json(&path.join("power1_average"), false);
        let power_input = sysfs::numeric_json(&path.join("power1_input"), false);
        if key(&path)? != before || sysfs::bounded_text(&path.join("name"))? != "amdgpu" {
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

fn ras_counters(text: &str) -> Result<(u64, u64), &'static str> {
    let (mut ce, mut ue) = (None, None);
    for line in text.lines() {
        let (label, value) = line.split_once(':').ok_or("invalid")?;
        let value = value.trim();
        if !sysfs::name_digits(value) {
            return Err("invalid");
        }
        let dest = match label {
            "ce" => &mut ce,
            "ue" => &mut ue,
            _ => return Err("invalid"),
        };
        if dest
            .replace(value.parse::<u64>().map_err(|_| "invalid")?)
            .is_some()
        {
            return Err("invalid");
        }
    }
    Ok((ce.ok_or("invalid")?, ue.ok_or("invalid")?))
}

fn read_umc_ras(
    bdf: &str,
    index: &Path,
    devices: &Path,
    drivers: &Path,
) -> Result<(u64, u64), &'static str> {
    let device = inventory::pci_entry(index, devices, bdf)?.ok_or("unavailable")?;
    if inventory::pci_attr(&device.0.join("vendor"))? != 0x1002 {
        return Err("unsupported");
    }
    let driver = amdgpu_binding(&device.0, drivers)?;
    let ras = device.0.join("ras");
    let before = key(&ras)?;
    let value = ras_counters(&sysfs::bounded_text(&ras.join("umc_err_count"))?)?;
    if key(&ras)? != before
        || amdgpu_binding(&device.0, drivers) != Ok(driver)
        || inventory::pci_entry(index, devices, bdf)? != Some(device)
    {
        return Err("changed");
    }
    Ok(value)
}

pub(crate) fn snapshot_umc_ras(device: &str) -> Result<String, String> {
    let bdf = pcie::bdf(device)?;
    let data = read_umc_ras(
        &bdf,
        Path::new("/sys/bus/pci/devices"),
        Path::new("/sys/devices"),
        Path::new("/sys/bus/pci/drivers"),
    );
    let (status, counters) = match data {
        Ok((ce, ue)) => (
            "observed",
            format!(r#","corrected_errors":"{ce}","uncorrected_errors":"{ue}""#),
        ),
        Err(status) => (status, String::new()),
    };
    Ok(format!(
        r#"{{"schema_version":1,"kind":"amdgpu_umc_ras_observation","bdf":"{bdf}","status":"{status}"{counters}}}"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn ras_rejects_unknown_duplicate_missing_or_overflowed_counts() {
        assert_eq!(ras_counters("ce:0\nue:0"), Ok((0, 0)));
        for bad in [
            "ce: 0",
            "ce: 0\nce: 2",
            "ue: -1\nce: 0",
            "ue: 0\nce: 18446744073709551616",
            "ue: 0\nce: 0\nother: 1",
        ] {
            assert_eq!(ras_counters(bad), Err("invalid"));
        }
    }

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
        let ras = dev.join("ras");
        fs::create_dir(&ras).unwrap();
        assert_eq!(
            read_umc_ras(bdf, &index, &devices, &drivers),
            Err("unavailable")
        );
        fs::write(ras.join("umc_err_count"), "ue: 0\nce: 9007199254740993\n").unwrap();
        assert_eq!(
            read_umc_ras(bdf, &index, &devices, &drivers),
            Ok((9007199254740993, 0))
        );
        fs::remove_file(ras.join("umc_err_count")).unwrap();
        symlink(sensor.join("temp1_input"), ras.join("umc_err_count")).unwrap();
        assert_eq!(
            read_umc_ras(bdf, &index, &devices, &drivers),
            Err("invalid")
        );
        fs::remove_file(&driver_link).unwrap();
        symlink(&sensor, &driver_link).unwrap();
        assert!(render(bdf, &index, &devices, &drivers).contains(r#""status":"unsupported""#));
        assert!(render("0000:09:00.0", &index, &devices, &drivers)
            .contains(r#""status":"unavailable""#));
    }
}

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

fn bound_to_amdgpu(target: &Path, drivers: &Path) -> bool {
    let link = target.join("driver");
    let expected = drivers.join("amdgpu");
    fs::symlink_metadata(&link).map_or(false, |m| m.is_symlink())
        && fs::canonicalize(&link).map_or(false, |p| p == expected)
        && key(&expected).is_ok()
}

fn scan(
    bdf: &str,
    index: &Path,
    devices: &Path,
    drivers: &Path,
) -> Result<Vec<String>, &'static str> {
    let device = inventory::pci_entry(index, devices, bdf)?.ok_or("unavailable")?;
    if inventory::pci_attr(&device.0.join("vendor"))? != 0x1002
        || !bound_to_amdgpu(&device.0, drivers)
    {
        return Err("unsupported");
    }
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
        || !bound_to_amdgpu(&device.0, drivers)
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

//! Read-only locality hints for one explicitly selected PCI function.
use crate::pcie::bdf;
use crate::pcie_health::json_string;
use crate::sysfs::{attribute_status, bounded_text};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

fn node_label(value: &str) -> Result<u32, &'static str> {
    if value == "-1" {
        return Err("unknown");
    }
    let n = value.parse::<u32>().map_err(|_| "invalid")?;
    if value == n.to_string() && n <= i32::MAX as u32 {
        Ok(n)
    } else {
        Err("invalid")
    }
}

fn report(index: &Path, devices: &Path, selected: &str) -> Result<String, String> {
    let id = bdf(selected)?;
    let by_id = index.join(&id);
    let identity = || {
        let target = fs::canonicalize(&by_id).map_err(attribute_status)?;
        if !target.starts_with(devices)
            || target.file_name().and_then(|v| v.to_str()) != Some(id.as_str())
        {
            return Err("invalid");
        }
        let meta = fs::symlink_metadata(&target).map_err(attribute_status)?;
        if !meta.is_dir() {
            return Err("invalid");
        }
        Ok((target, meta.dev(), meta.ino()))
    };
    let device = json_string(&id);
    let withheld = |status| {
        format!(
            r#"{{"schema_version":1,"kind":"pcie_locality","device":{device},"status":"{status}"}}"#
        )
    };
    let first = match identity() {
        Ok(v) => v,
        Err(e) => return Ok(withheld(e)),
    };
    let node = match bounded_text(&first.0.join("numa_node")).and_then(|s| node_label(&s)) {
        Ok(n) => format!(r#"{{"status":"observed","value":{n}}}"#),
        Err(e) => format!(r#"{{"status":"{e}"}}"#),
    };
    let cpus = match crate::numa::local_cpu_list(&first.0.join("local_cpulist")) {
        Ok(ids) => format!(r#"{{"status":"observed","value":{ids:?}}}"#),
        Err(e) => format!(r#"{{"status":"{e}"}}"#),
    };
    if identity().ok().as_ref() != Some(&first) {
        return Ok(withheld("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"kind":"pcie_locality","device":{device},"status":"observed","numa_node":{node},"local_cpu_ids":{cpus}}}"#
    ))
}

pub(crate) fn run(selected: &str) -> Result<(), String> {
    println!(
        "{}",
        report(
            Path::new("/sys/bus/pci/devices"),
            Path::new("/sys/devices"),
            selected
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn local_hints_distinguish_node_zero_from_unknown_and_bound_cpu_list() {
        let root = std::env::temp_dir().join(format!("eld-pci-locality-{}", std::process::id()));
        let devices = root.join("devices");
        let index = root.join("index");
        let id = "0000:01:00.0";
        let device = devices.join("pcie").join(id);
        fs::create_dir_all(&device).unwrap();
        fs::create_dir(&index).unwrap();
        symlink(&device, index.join(id)).unwrap();
        fs::write(device.join("numa_node"), "0\n").unwrap();
        fs::write(device.join("local_cpulist"), "0-1,8\n").unwrap();
        let sample = || report(&index, &devices, id).unwrap();
        let observed = sample();
        assert!(observed.contains(r#""numa_node":{"status":"observed","value":0}"#));
        assert!(observed.contains(r#""local_cpu_ids":{"status":"observed","value":[0, 1, 8]}"#));
        fs::write(device.join("numa_node"), "-1\n").unwrap();
        fs::write(device.join("local_cpulist"), "0-8192\n").unwrap();
        let unknown = sample();
        assert!(unknown.contains(r#""numa_node":{"status":"unknown"}"#));
        assert!(unknown.contains(r#""local_cpu_ids":{"status":"invalid"}"#));
        assert!(report(&index, &devices, "../01:00.0").is_err());
        assert!(report(&index, &devices, "0000:02:00.0")
            .unwrap()
            .contains(r#""status":"unavailable""#));
        fs::remove_dir_all(root).unwrap();
    }
}

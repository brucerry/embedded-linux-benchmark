//! Observe fixed kernel counters for one logical CPU, never MSRs or new load.
use crate::sysfs::{attribute_status, numeric_json};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub(crate) const MAX_CPU: u64 = 9_999_999;

fn report(root: &Path, cpu: u64) -> Result<String, String> {
    if cpu > MAX_CPU {
        return Err("CPU index outside supported range".to_owned());
    }
    let dir = root.join(format!("cpu{cpu}"));
    let attrs = dir.join("thermal_throttle");
    let id = |p: &Path| {
        let m = fs::symlink_metadata(p).map_err(attribute_status)?;
        if !m.is_dir() {
            return Err("invalid");
        }
        Ok((m.dev(), m.ino()))
    };
    let source = || Ok::<_, &str>((id(&dir)?, id(&attrs)?));
    let base = |status| {
        format!(
            r#"{{"schema_version":1,"kind":"cpu_thermal_throttle","cpu":{cpu},"status":"{status}"}}"#
        )
    };
    let initial = match source() {
        Ok(value) => value,
        Err(status) => return Ok(base(status)),
    };
    let core = numeric_json(&attrs.join("core_throttle_count"), false);
    let package = numeric_json(&attrs.join("package_throttle_count"), false);
    if source().ok() != Some(initial) {
        return Ok(base("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"kind":"cpu_thermal_throttle","cpu":{cpu},"status":"observed","core_throttle_count":{core},"package_throttle_count":{package}}}"#
    ))
}

pub(crate) fn run(cpu: u64) -> Result<(), String> {
    println!("{}", report(Path::new("/sys/devices/system/cpu"), cpu)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn zero_throttle_counts_and_absent_abi_are_distinct() {
        let root = std::env::temp_dir().join(format!("eld-throttle-{}", std::process::id()));
        let p = root.join("cpu0/thermal_throttle");
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("core_throttle_count"), "0\n").unwrap();
        fs::write(p.join("package_throttle_count"), "18446744073709551615\n").unwrap();
        let data = report(&root, 0).unwrap();
        assert!(data.contains(r#""core_throttle_count":{"status":"observed","value":"0"}"#));
        assert!(data.contains(
            r#""package_throttle_count":{"status":"observed","value":"18446744073709551615"}"#
        ));
        fs::remove_file(p.join("package_throttle_count")).unwrap();
        assert!(report(&root, 0)
            .unwrap()
            .contains(r#""package_throttle_count":{"status":"unavailable"}"#));
        symlink(
            p.join("core_throttle_count"),
            p.join("package_throttle_count"),
        )
        .unwrap();
        assert!(report(&root, 0)
            .unwrap()
            .contains(r#""package_throttle_count":{"status":"invalid"}"#));
        symlink(root.join("cpu0"), root.join("cpu1")).unwrap();
        assert!(report(&root, 1).unwrap().contains(r#""status":"invalid""#));
        assert!(report(&root, 2)
            .unwrap()
            .contains(r#""status":"unavailable""#));
        assert!(report(&root, MAX_CPU + 1).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

//! Passive, selected Linux thermal cooling-device state.
use crate::pcie_health::json_string;
use crate::sysfs::{attribute_status, bounded_text, numeric_json};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

fn report(class: &Path, devices: &Path, name: &str) -> Result<String, String> {
    let Some(index) = name.strip_prefix("cooling_device") else {
        return Err("expected cooling_deviceN".to_owned());
    };
    if index.is_empty()
        || index.len() > 7
        || (index.len() > 1 && index.starts_with('0'))
        || !index.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("expected a canonical cooling_deviceN".to_owned());
    }
    let entry = class.join(name);
    let identify = || {
        let p = fs::canonicalize(&entry).map_err(attribute_status)?;
        if !p.starts_with(devices) || p.file_name().and_then(|s| s.to_str()) != Some(name) {
            return Err("invalid");
        }
        let m = fs::symlink_metadata(&p).map_err(attribute_status)?;
        if !m.is_dir() {
            return Err("invalid");
        }
        Ok((p, m.dev(), m.ino()))
    };
    let base = |status| {
        format!(
            r#"{{"schema_version":1,"kind":"thermal_cooling_state","device":"{name}","status":"{status}"}}"#
        )
    };
    let initial = match identify() {
        Ok(id) => id,
        Err(status) => return Ok(base(status)),
    };
    let label = bounded_text(&initial.0.join("type")).and_then(|s| {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
            Ok(s)
        } else {
            Err("invalid")
        }
    });
    let label = match label {
        Ok(value) => format!(r#"{{"status":"observed","value":{}}}"#, json_string(&value)),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    };
    let current = numeric_json(&initial.0.join("cur_state"), false);
    let maximum = numeric_json(&initial.0.join("max_state"), false);
    if identify().ok().as_ref() != Some(&initial) {
        return Ok(base("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"kind":"thermal_cooling_state","device":"{name}","status":"observed","type":{label},"cur_state":{current},"max_state":{maximum}}}"#
    ))
}

pub(crate) fn run(name: &str) -> Result<(), String> {
    println!(
        "{}",
        report(
            Path::new("/sys/class/thermal"),
            Path::new("/sys/devices"),
            name
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn reports_selected_ordinal_states_separately_and_refuses_aliases() {
        let root = std::env::temp_dir().join(format!("eld-cooling-{}", std::process::id()));
        let class = root.join("class");
        let devices = root.join("devices");
        let d = devices.join("virtual/thermal/cooling_device0");
        fs::create_dir_all(&class).unwrap();
        fs::create_dir_all(&d).unwrap();
        symlink(&d, class.join("cooling_device0")).unwrap();
        fs::write(d.join("type"), "thermal-test\n").unwrap();
        fs::write(d.join("cur_state"), "0\n").unwrap();
        fs::write(d.join("max_state"), "8\n").unwrap();
        let observe = || report(&class, &devices, "cooling_device0").unwrap();
        let output = observe();
        assert!(output.contains(r#""type":{"status":"observed","value":"thermal-test"}"#));
        assert!(output.contains(r#""cur_state":{"status":"observed","value":"0"}"#));
        assert!(output.contains(r#""max_state":{"status":"observed","value":"8"}"#));
        fs::remove_file(d.join("max_state")).unwrap();
        assert!(observe().contains(r#""max_state":{"status":"unavailable"}"#));
        symlink(d.join("cur_state"), d.join("max_state")).unwrap();
        assert!(observe().contains(r#""max_state":{"status":"invalid"}"#));
        assert!(report(&class, &devices, "../cooling_device0").is_err());
        assert!(report(&class, &devices, "cooling_device00").is_err());
        assert!(report(&class, &devices, "cooling_device1")
            .unwrap()
            .contains(r#""status":"unavailable""#));
        fs::remove_dir_all(root).unwrap();
    }
}

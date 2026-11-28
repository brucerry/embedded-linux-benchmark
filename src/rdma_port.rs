//! Explicit local RDMA port observations: no enumeration, traffic or writes.
use crate::pcie_health::json_string;
use crate::sysfs::{attribute_status, bounded_text};
use std::fs;
use std::path::{Path, PathBuf};

const FIELDS: [&str; 4] = ["state", "phys_state", "link_layer", "rate"];

fn selectable(device: &str) -> bool {
    !device.is_empty()
        && device.len() <= 63
        && device.as_bytes()[0].is_ascii_alphanumeric()
        && device
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn selected_path(root: &Path, device: &str, port: u64) -> Result<PathBuf, &'static str> {
    let device =
        fs::canonicalize(root.join("class/infiniband").join(device)).map_err(attribute_status)?;
    if !device.starts_with(root.join("devices")) || device == root.join("devices") {
        return Err("invalid");
    }
    let name = port.to_string();
    let path = device.join("ports").join(&name);
    let canonical = fs::canonicalize(&path).map_err(attribute_status)?;
    if canonical != path || !canonical.is_dir() {
        return Err("invalid");
    }
    Ok(canonical)
}

fn json_reading(path: &Path) -> String {
    match bounded_text(path) {
        Ok(text)
            if !text.is_empty()
                && text.len() <= 64
                && text.bytes().all(|b| b.is_ascii_graphic() || b == b' ') =>
        {
            format!(r#"{{"status":"reported","raw":{}}}"#, json_string(&text))
        }
        Ok(_) => r#"{"status":"invalid"}"#.into(),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

pub(crate) fn snapshot(root: &Path, device: &str, port: u64) -> Result<String, String> {
    if !selectable(device) || port > u64::from(u32::MAX) {
        return Err("expected a valid explicit RDMA device name and port number".into());
    }
    let (status, attributes) = match selected_path(root, device, port) {
        Ok(selected) => {
            let fields: Vec<String> = FIELDS
                .iter()
                .map(|field| format!(r#""{field}":{}"#, json_reading(&selected.join(field))))
                .collect();
            ("observed", format!("{{{}}}", fields.join(",")))
        }
        Err(status) => (status, "{}".into()),
    };
    Ok(format!(
        concat!(
            r#"{{"schema_version":1,"kind":"passive_rdma_port","source":"linux_sysfs","#,
            r#""selection":{{"device":{},"port":{}}},"status":"{status}","attributes":{},"#,
            r#""rate_scope":"kernel_reported_rdma","ethernet_line_rate":"not_assessed","#,
            r#""hardware_provenance":"not_assessed","throughput_measured":false,"traffic_started":false}}"#
        ),
        json_string(device),
        port,
        attributes,
        status = status,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::{env, fs, process, time::SystemTime};

    #[test]
    fn observes_only_the_selected_port_and_preserves_software_roce_labels() {
        let suffix = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("eld-rdma-port-{}-{suffix}", process::id()));
        let target = root.join("devices/virtual/infiniband/rxe0");
        let port = target.join("ports/1");
        fs::create_dir_all(&port).unwrap();
        fs::create_dir_all(root.join("class/infiniband")).unwrap();
        symlink(&target, root.join("class/infiniband/rxe0")).unwrap();
        for (field, value) in [
            ("state", "4: ACTIVE"),
            ("phys_state", "5: LinkUp"),
            ("link_layer", "Ethernet"),
            ("rate", "2.5 Gb/sec (1X SDR)"),
        ] {
            let fixture = include_str!("../tests/fixtures/rdma-rxe-fallback.json");
            assert!(fixture.contains(&format!(r#""{field}": "{value}""#)));
            fs::write(port.join(field), format!("{value}\n")).unwrap();
        }
        let result = snapshot(&root, "rxe0", 1).unwrap();
        assert_eq!(
            result,
            include_str!("../tests/fixtures/rdma-passive-rxe-report.json").trim_end()
        );
        assert!(result.contains(r#""status":"observed""#));
        assert!(result.contains(r#""raw":"2.5 Gb/sec (1X SDR)""#));
        assert!(result.contains(r#""ethernet_line_rate":"not_assessed""#));
        assert!(result.contains(r#""hardware_provenance":"not_assessed""#));
        for field in ["state", "phys_state", "link_layer"] {
            assert!(result.contains(&format!(r#""{field}":{{"status":"reported""#)));
        }
        assert!(snapshot(&root, "rxe0", 2)
            .unwrap()
            .contains(r#""status":"unavailable""#));
        assert!(snapshot(&root, "../rxe0", 1).is_err());
        symlink(root.join("class"), root.join("class/infiniband/foreign")).unwrap();
        assert!(snapshot(&root, "foreign", 1)
            .unwrap()
            .contains(r#""status":"invalid""#));
        fs::remove_file(port.join("rate")).unwrap();
        symlink(port.join("state"), port.join("rate")).unwrap();
        assert!(snapshot(&root, "rxe0", 1)
            .unwrap()
            .contains(r#""rate":{"status":"invalid"}"#));
        fs::remove_dir_all(root).unwrap();
    }
}

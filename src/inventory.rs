use crate::{pcie_health::json_string, sysfs::open_attribute};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

const LIMIT: u64 = 4096;
const OS_KEYS: [&str; 2] = ["ID", "VERSION_ID"];

#[derive(Debug, PartialEq, Eq)]
enum Observation {
    Available(String),
    Unavailable,
    Inaccessible,
    Invalid,
}

impl Observation {
    fn error(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::Unavailable,
            io::ErrorKind::PermissionDenied => Self::Inaccessible,
            _ => Self::Invalid,
        }
    }

    fn json(&self) -> String {
        match self {
            Self::Available(value) => {
                format!(r#"{{"status":"observed","value":{}}}"#, json_string(value))
            }
            Self::Unavailable => r#"{"status":"unavailable"}"#.into(),
            Self::Inaccessible => r#"{"status":"inaccessible"}"#.into(),
            Self::Invalid => r#"{"status":"invalid"}"#.into(),
        }
    }
}

fn bounded(path: &Path) -> Result<String, io::Error> {
    let file = open_attribute(path)?;
    let mut text = String::new();
    file.take(LIMIT + 1).read_to_string(&mut text)?;
    if text.len() as u64 > LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "attribute too large",
        ));
    }
    Ok(text)
}

fn text_value(input: &str) -> Option<String> {
    // The device tree represents model strings as NUL-terminated properties.
    let value = input.trim_end_matches('\0').trim();
    (!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

fn text(path: &str) -> Observation {
    match bounded(Path::new(path)) {
        Ok(value) => text_value(&value).map_or(Observation::Invalid, Observation::Available),
        Err(e) => Observation::error(e),
    }
}

fn field(release: &str, key: &str) -> Observation {
    let mut value = None;
    for row in release.lines() {
        let Some((name, raw)) = row.split_once('=') else {
            continue;
        };
        if name != key {
            continue;
        }
        if value.is_some() {
            return Observation::Invalid;
        }
        let raw = raw.trim();
        let unquoted = if raw.starts_with('"') {
            raw.strip_prefix('"').and_then(|v| v.strip_suffix('"'))
        } else if raw.starts_with('\'') {
            raw.strip_prefix('\'').and_then(|v| v.strip_suffix('\''))
        } else {
            Some(raw)
        };
        let observed = unquoted.filter(|v| {
            !v.is_empty()
                && v.len() <= 96
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        });
        match observed {
            Some(v) => value = Some(v.to_owned()),
            None => return Observation::Invalid,
        }
    }
    value.map_or(Observation::Unavailable, Observation::Available)
}

fn release() -> [Observation; 2] {
    let candidate = Path::new("/etc/os-release");
    let actual = match fs::canonicalize(candidate) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Path::new("/usr/lib/os-release").to_owned()
        }
        Err(e) => {
            let k = e.kind();
            return OS_KEYS.map(|_| Observation::error(io::Error::from(k)));
        }
    };
    if actual != candidate && actual != Path::new("/usr/lib/os-release") {
        return OS_KEYS.map(|_| Observation::Invalid);
    }
    match bounded(&actual) {
        Ok(value) => OS_KEYS.map(|key| field(&value, key)),
        Err(e) => {
            let k = e.kind();
            OS_KEYS.map(|_| Observation::error(io::Error::from(k)))
        }
    }
}

const MAX_REQUIRED_PCIE: usize = 16;

fn required_bdfs(list: Option<&str>) -> Result<Vec<String>, String> {
    let Some(list) = list else {
        return Ok(Vec::new());
    };
    if list.len() > MAX_REQUIRED_PCIE * 13 {
        return Err("require-pcie accepts at most 16 complete PCI device IDs".into());
    }
    let mut out = Vec::new();
    for item in list.split(',') {
        let id = crate::pcie::bdf(item)?;
        if out.len() == MAX_REQUIRED_PCIE || out.contains(&id) {
            return Err("require-pcie needs at most 16 unique PCI device IDs".into());
        }
        out.push(id);
    }
    Ok(out)
}

fn pci_status(index: &Path, sys_devices: &Path, bdf: &str) -> &'static str {
    use std::os::unix::fs::MetadataExt;
    let index_identity = || match fs::symlink_metadata(index) {
        Ok(m) if m.is_dir() => Ok((m.dev(), m.ino())),
        Ok(_) => Err("invalid"),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err("inaccessible"),
        Err(_) => Err("unavailable"),
    };
    let initial_index = match index_identity() {
        Ok(id) => id,
        Err(status) => return status,
    };
    let path = index.join(bdf);
    let read = || -> Result<Option<(std::path::PathBuf, u64, u64)>, &'static str> {
        match fs::symlink_metadata(&path) {
            Ok(m) if m.is_symlink() => {}
            Ok(_) => return Err("invalid"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err("inaccessible"),
            Err(_) => return Err("unavailable"),
        }
        let target = match fs::canonicalize(&path) {
            Ok(p) => p,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return Err("inaccessible"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err("changed"),
            Err(_) => return Err("invalid"),
        };
        if !target.starts_with(sys_devices)
            || target.file_name().and_then(|x| x.to_str()) != Some(bdf)
        {
            return Err("invalid");
        }
        match fs::symlink_metadata(&target) {
            Ok(m) if m.is_dir() => Ok(Some((target, m.dev(), m.ino()))),
            Ok(_) => Err("invalid"),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err("changed"),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err("inaccessible"),
            Err(_) => Err("unavailable"),
        }
    };
    let first = read();
    let second = read();
    if index_identity() != Ok(initial_index) || first != second {
        return "changed";
    }
    match second {
        Ok(Some(_)) => "present",
        Ok(None) => "missing",
        Err(status) => status,
    }
}

pub(crate) fn run(required: Option<&str>) -> Result<(), String> {
    let required = required_bdfs(required)?;
    let [distribution, version] = release();
    let cells = [
        ("distribution_id", distribution),
        ("distribution_version", version),
        ("kernel_release", text("/proc/sys/kernel/osrelease")),
        (
            "device_tree_model",
            text("/sys/firmware/devicetree/base/model"),
        ),
        ("dmi_system_model", text("/sys/class/dmi/id/product_name")),
        ("dmi_board_model", text("/sys/class/dmi/id/board_name")),
        (
            "dmi_firmware_version",
            text("/sys/class/dmi/id/bios_version"),
        ),
    ];
    let values = cells
        .iter()
        .map(|(key, value)| format!("{}:{}", json_string(key), value.json()))
        .collect::<Vec<_>>()
        .join(",");
    let profile = if required.is_empty() {
        String::new()
    } else {
        let rows = required
            .iter()
            .map(|bdf| {
                format!(
                    r#"{{"bdf":{},"status":"{}"}}"#,
                    json_string(bdf),
                    pci_status(
                        Path::new("/sys/bus/pci/devices"),
                        Path::new("/sys/devices"),
                        bdf
                    ),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#","required_pcie":[{rows}]"#)
    };
    println!(
        r#"{{"schema_version":1,"process_architecture":{},{values}{profile}}}"#,
        json_string(std::env::consts::ARCH)
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_pcie_checks_bounds_and_bus_availability() {
        use std::os::unix::fs::symlink;
        assert!(required_bdfs(Some("0000:0A:00.0,0000:0a:00.0")).is_err());
        assert!(required_bdfs(Some("../outside")).is_err());
        assert!(required_bdfs(Some(&"x".repeat(209))).is_err());
        assert_eq!(
            required_bdfs(Some("0000:0A:00.0")).unwrap(),
            ["0000:0a:00.0"]
        );
        let root = std::env::temp_dir().join(format!("eld-inventory-pcie-{}", std::process::id()));
        let index = root.join("bus/pci/devices");
        let devices = root.join("devices");
        let id = "0000:01:00.0";
        let path = index.join(id);
        let dst = devices.join("pci0000:00").join(id);
        assert_eq!(pci_status(&index, &devices, id), "unavailable");
        fs::create_dir_all(&index).unwrap();
        assert_eq!(pci_status(&index, &devices, id), "missing");
        symlink(&dst, &path).unwrap();
        assert_eq!(pci_status(&index, &devices, id), "changed");
        fs::create_dir_all(&dst).unwrap();
        assert_eq!(pci_status(&index, &devices, id), "present");
        fs::remove_file(&path).unwrap();
        symlink(&index, &path).unwrap();
        assert_eq!(pci_status(&index, &devices, id), "invalid");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn release_fields_are_data_never_shell_and_duplicates_are_refused() {
        let sample = "ID=openwrt\nVERSION_ID='23.05.6'\nNAME=Other";
        assert_eq!(
            field(sample, "ID"),
            Observation::Available("openwrt".into())
        );
        assert_eq!(
            field(sample, "VERSION_ID"),
            Observation::Available("23.05.6".into())
        );
        assert_eq!(field("ID=linux\nID=other", "ID"), Observation::Invalid);
        assert_eq!(field("ID=$(fake-command)", "ID"), Observation::Invalid);
        assert_eq!(field("ID=\"unterminated", "ID"), Observation::Invalid);
        assert_eq!(field("NAME=linux", "ID"), Observation::Unavailable);
    }

    #[test]
    fn device_tree_model_accepts_nul_terminator_not_control_input() {
        assert_eq!(text_value("Router A\0"), Some("Router A".into()));
        assert_eq!(text_value("Router\0B"), None);
        assert_eq!(text_value("foo\nother"), None);
        assert_eq!(text_value(""), None);
        assert_eq!(text_value(&"m".repeat(257)), None);
    }
}

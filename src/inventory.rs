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

pub(crate) fn run() {
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
    println!(
        r#"{{"schema_version":1,"process_architecture":{},{values}}}"#,
        json_string(std::env::consts::ARCH)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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

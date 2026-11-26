use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

pub(crate) fn open_attribute(path: &Path) -> io::Result<File> {
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a regular sysfs attribute",
        ));
    }
    let file = OpenOptions::new().read(true).open(path)?;
    let after = file.metadata()?;
    use std::os::unix::fs::MetadataExt;
    if !after.file_type().is_file() || (before.dev(), before.ino()) != (after.dev(), after.ino()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "sysfs attribute changed during open",
        ));
    }
    Ok(file)
}

pub(crate) fn name_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn attribute_status(e: io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    }
}

pub(crate) fn bounded_text(path: &Path) -> Result<String, &'static str> {
    let mut raw = String::new();
    open_attribute(path)
        .map_err(attribute_status)?
        .take(129)
        .read_to_string(&mut raw)
        .map_err(attribute_status)?;
    if raw.len() > 128 {
        return Err("invalid");
    }
    Ok(raw.trim().to_owned())
}

pub(crate) fn numeric_json(path: &Path, signed: bool) -> String {
    let value = bounded_text(path).and_then(|raw| {
        let digits = if signed {
            raw.strip_prefix('-').unwrap_or(&raw)
        } else {
            &raw
        };
        if !name_digits(digits) {
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

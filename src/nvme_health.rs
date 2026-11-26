use crate::{pcie_health::json_string, sysfs::open_attribute};
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

fn valid_controller(name: &str) -> bool {
    let Some(tail) = name.strip_prefix("nvme") else {
        return false;
    };
    !tail.is_empty()
        && tail.len() <= 7
        && tail.bytes().all(|c| c.is_ascii_digit())
        && (tail.len() == 1 || !tail.starts_with('0'))
}

fn io_status(e: io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::Unsupported => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    }
}

fn attribute(path: &Path) -> String {
    let result = (|| {
        let mut s = String::new();
        open_attribute(path)
            .map_err(io_status)?
            .take(129)
            .read_to_string(&mut s)
            .map_err(io_status)?;
        let text = s.trim();
        if s.len() > 128
            || text.is_empty()
            || !text.bytes().all(|c| c.is_ascii_graphic() || c == b' ')
        {
            return Err("invalid");
        }
        Ok(json_string(text))
    })();
    match result {
        Ok(v) => format!(r#"{{"status":"observed","value":{v}}}"#),
        Err(s) => format!(r#"{{"status":"{s}"}}"#),
    }
}

fn report(index: &Path, devices: &Path, name: &str) -> Result<String, String> {
    if !valid_controller(name) {
        return Err("expected a named NVMe controller like nvme0, not a namespace".into());
    }
    let entry = index.join(name);
    let identity = || {
        let location = fs::canonicalize(&entry).map_err(io_status)?;
        if !location.starts_with(devices)
            || location.file_name().and_then(|v| v.to_str()) != Some(name)
        {
            return Err("invalid");
        }
        let meta = fs::symlink_metadata(&location).map_err(io_status)?;
        if !meta.is_dir() {
            return Err("invalid");
        }
        Ok((location, meta.dev(), meta.ino()))
    };
    let controller = json_string(name);
    let withheld = |status| {
        format!(
            r#"{{"schema_version":1,"controller":{controller},"status":"{status}","smart_sampled":false}}"#
        )
    };
    let initial = match identity() {
        Ok(id) => id,
        Err(status) => return Ok(withheld(status)),
    };
    let data = ["state", "transport", "model", "firmware_rev"]
        .map(|leaf| attribute(&initial.0.join(leaf)));
    if identity().ok().as_ref() != Some(&initial) {
        return Ok(withheld("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"controller":{controller},"status":"observed","state":{},"transport":{},"model":{},"firmware_revision":{},"smart_sampled":false}}"#,
        data[0], data[1], data[2], data[3]
    ))
}

pub(crate) fn run(name: &str) -> Result<(), String> {
    println!(
        "{}",
        report(
            Path::new("/sys/class/nvme"),
            Path::new("/sys/devices"),
            name
        )?
    );
    Ok(())
}

const SMART_PAGE_BYTES: usize = 512;

// NVMe SMART / Health Information log page 02h. This parser does not
// establish the supplied page's provenance, age, or controller identity.
fn decode_smart(input: impl Read) -> Result<String, String> {
    let mut page = Vec::with_capacity(SMART_PAGE_BYTES + 1);
    input
        .take((SMART_PAGE_BYTES + 1) as u64)
        .read_to_end(&mut page)
        .map_err(|e| format!("cannot read NVMe SMART page from stdin: {e}"))?;
    if page.len() != SMART_PAGE_BYTES {
        return Err(
            "expected exactly 512 bytes from one saved NVMe SMART log page on stdin".into(),
        );
    }

    let count = |offset| {
        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&page[offset..offset + 16]);
        u128::from_le_bytes(bytes)
    };
    let bounded_percent = |v: u8| {
        if v <= 100 {
            v.to_string()
        } else {
            "null".into()
        }
    };
    let kelvin = u16::from_le_bytes([page[1], page[2]]);
    let kelvin = match kelvin {
        0 => "null".into(),
        value => value.to_string(),
    };
    Ok(format!(
        concat!(
            "{{\"schema_version\":1,\"source\":\"provided_smart_log_page\",",
            "\"device_sampled\":false,\"critical_warning_raw\":{},",
            "\"composite_temperature_kelvin\":{},\"available_spare_percent\":{},",
            "\"available_spare_threshold_percent\":{},\"percent_used_raw\":{},",
            "\"power_on_hours_raw\":\"{}\",\"unsafe_shutdowns_raw\":\"{}\",",
            "\"media_errors_raw\":\"{}\",\"error_log_entries_raw\":\"{}\"}}",
        ),
        page[0],
        kelvin,
        bounded_percent(page[3]),
        bounded_percent(page[4]),
        page[5],
        count(128),
        count(144),
        count(160),
        count(176)
    ))
}

pub(crate) fn decode_stdin() -> Result<(), String> {
    println!("{}", decode_smart(io::stdin().lock())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn observes_whitelisted_metadata_without_smart_serial_or_reset() {
        let fixture = std::env::temp_dir().join(format!("eld-nvme-{}", std::process::id()));
        let index = fixture.join("index");
        let devices = fixture.join("devices");
        let target = devices.join("pci/nvme/nvme0");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir(&index).unwrap();
        symlink(&target, index.join("nvme0")).unwrap();
        fs::write(target.join("state"), "live\n").unwrap();
        fs::write(target.join("model"), " Fixture Drive  \n").unwrap();
        fs::write(target.join("serial"), "ignored-fixture-secret\n").unwrap();
        fs::write(target.join("reset_controller"), "never changed\n").unwrap();
        fs::write(target.join("firmware_rev"), "bad\tvalue").unwrap();
        let value = report(&index, &devices, "nvme0").unwrap();
        assert!(value.contains(r#""state":{"status":"observed","value":"live"}"#));
        assert!(value.contains(r#""model":{"status":"observed","value":"Fixture Drive"}"#));
        assert!(value.contains(r#""transport":{"status":"unavailable"}"#));
        assert!(value.contains(r#""firmware_revision":{"status":"invalid"}"#));
        assert!(value.contains(r#""smart_sampled":false"#));
        assert!(!value.contains("ignored-fixture-secret"));
        assert_eq!(
            fs::read_to_string(target.join("reset_controller")).unwrap(),
            "never changed\n"
        );
        assert!(report(&index, &devices, "nvme0n1").is_err());
        symlink(&target, index.join("nvme1")).unwrap();
        assert!(report(&index, &devices, "nvme1")
            .unwrap()
            .contains(r#""status":"invalid""#));
        fs::remove_dir_all(fixture).unwrap();
    }
}

use crate::{pcie_health::json_string, sysfs::open_attribute};
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

fn interface_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 15
        && !matches!(name, "." | "..")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

fn reason(e: io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::Unsupported => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ if matches!(e.raw_os_error(), Some(22 | 95)) => "unavailable",
        _ => "invalid",
    }
}

fn read(path: &Path, format: fn(&str) -> Result<String, &'static str>) -> String {
    let result = (|| {
        let mut text = String::new();
        open_attribute(path)
            .map_err(reason)?
            .take(65)
            .read_to_string(&mut text)
            .map_err(reason)?;
        if text.len() > 64 {
            return Err("invalid");
        }
        format(text.trim())
    })();
    match result {
        Ok(v) => format!(r#"{{"status":"observed","value":{v}}}"#),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

fn decimal(s: &str) -> Result<String, &'static str> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err("invalid");
    }
    s.parse::<u64>()
        .map(|v| json_string(&v.to_string()))
        .map_err(|_| "invalid")
}

fn speed(s: &str) -> Result<String, &'static str> {
    if matches!(s, "-1" | "0" | "4294967295") {
        Err("unavailable")
    } else {
        decimal(s)
    }
}

fn carrier(s: &str) -> Result<String, &'static str> {
    match s {
        "0" => Ok("false".into()),
        "1" => Ok("true".into()),
        _ => Err("invalid"),
    }
}

fn duplex(s: &str) -> Result<String, &'static str> {
    match s {
        "half" | "full" => Ok(json_string(s)),
        "unknown" => Err("unavailable"),
        _ => Err("invalid"),
    }
}

fn report(index: &Path, devices: &Path, name: &str) -> Result<String, String> {
    if !interface_name(name) {
        return Err("expected a Linux interface name of 1 to 15 safe characters".into());
    }
    let item = index.join(name);
    let identity = || {
        let path = fs::canonicalize(&item).map_err(reason)?;
        if !path.starts_with(devices)
            || path.file_name().and_then(|x| x.to_str()) != Some(name)
            || path
                .parent()
                .and_then(Path::file_name)
                .and_then(|x| x.to_str())
                != Some("net")
        {
            return Err("invalid");
        }
        let m = fs::symlink_metadata(&path).map_err(reason)?;
        if !m.is_dir() {
            return Err("invalid");
        }
        Ok((path, m.dev(), m.ino()))
    };
    let withheld = |status| {
        format!(
            r#"{{"schema_version":1,"interface":{},"status":"{status}"}}"#,
            json_string(name)
        )
    };
    let initial = match identity() {
        Ok(v) => v,
        Err(status) => return Ok(withheld(status)),
    };
    let path = &initial.0;
    let speed = read(&path.join("speed"), speed);
    let carrier = read(&path.join("carrier"), carrier);
    let duplex = read(&path.join("duplex"), duplex);
    let stats = path.join("statistics");
    let stats_identity = || match fs::symlink_metadata(&stats) {
        Ok(m) if m.is_dir() => Ok((m.dev(), m.ino())),
        Ok(_) => Err("invalid"),
        Err(e) => Err(reason(e)),
    };
    let previous_stats = stats_identity();
    let counters = [
        "rx_bytes",
        "tx_bytes",
        "rx_errors",
        "tx_errors",
        "rx_dropped",
        "tx_dropped",
        "carrier_changes",
    ]
    .into_iter()
    .map(|key| {
        let value = if key == "carrier_changes" {
            read(&path.join(key), decimal)
        } else if let Err(status) = previous_stats {
            format!(r#"{{"status":"{status}"}}"#)
        } else {
            read(&stats.join(key), decimal)
        };
        format!(r#""{key}":{value}"#)
    })
    .collect::<Vec<_>>()
    .join(",");
    if identity().ok().as_ref() != Some(&initial) || stats_identity() != previous_stats {
        return Ok(withheld("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"interface":{},"status":"observed","carrier":{carrier},"speed_mbps":{speed},"duplex":{duplex},"counters":{{{counters}}}}}"#,
        json_string(name)
    ))
}

pub(crate) fn run(name: &str) -> Result<(), String> {
    println!(
        "{}",
        report(Path::new("/sys/class/net"), Path::new("/sys/devices"), name)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn explicit_link_fixture_preserves_zero_and_unknown_link_speed() {
        let root = std::env::temp_dir().join(format!("eld-net-health-{}", std::process::id()));
        let index = root.join("index");
        let devices = root.join("devices");
        let link = devices.join("virtual/net/eth0");
        fs::create_dir_all(link.join("statistics")).unwrap();
        fs::create_dir(&index).unwrap();
        symlink(&link, index.join("eth0")).unwrap();
        fs::write(link.join("carrier"), "0\n").unwrap();
        fs::write(link.join("speed"), "-1\n").unwrap();
        fs::write(link.join("duplex"), "unknown\n").unwrap();
        fs::write(link.join("statistics/rx_errors"), "0\n").unwrap();
        fs::write(link.join("statistics/tx_errors"), "18446744073709551615\n").unwrap();
        let result = report(&index, &devices, "eth0").unwrap();
        assert!(result.contains(
            r#""carrier":{"status":"observed","value":false},"speed_mbps":{"status":"unavailable"}"#
        ));
        assert!(result.contains(r#""rx_errors":{"status":"observed","value":"0"},"tx_errors":{"status":"observed","value":"18446744073709551615"}"#));
        assert!(result.contains(r#""rx_dropped":{"status":"unavailable"}"#));
        assert!(report(&index, &devices, "../eth0").is_err());
        symlink(&link, index.join("eth1")).unwrap();
        assert!(report(&index, &devices, "eth1")
            .unwrap()
            .contains(r#""status":"invalid""#));
        fs::remove_dir_all(root).unwrap();
    }
}

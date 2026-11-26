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

type InterfaceIdentity = (std::path::PathBuf, u64, u64);

fn identity(index: &Path, devices: &Path, name: &str) -> Result<InterfaceIdentity, &'static str> {
    let item = index.join(name);
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
}

fn report(index: &Path, devices: &Path, name: &str) -> Result<String, String> {
    if !interface_name(name) {
        return Err("expected a Linux interface name of 1 to 15 safe characters".into());
    }
    let identity = || identity(index, devices, name);
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

pub(crate) const MAX_SAMPLE_MS: u64 = 60_000;

fn read_id(path: &Path) -> Option<String> {
    let mut s = String::new();
    open_attribute(path)
        .ok()?
        .take(65)
        .read_to_string(&mut s)
        .ok()?;
    if s.len() > 64 {
        return None;
    }
    Some(s.trim().to_owned())
}

fn boot_id(path: &Path) -> Option<String> {
    let value = read_id(path)?;
    (value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        }))
    .then_some(value)
}

fn sample_key(
    index: &Path,
    devices: &Path,
    boot: &Path,
    name: &str,
) -> Option<(InterfaceIdentity, u32, String)> {
    let first = identity(index, devices, name).ok()?;
    let id = read_id(&first.0.join("ifindex"))?.parse::<u32>().ok()?;
    if id == 0 || identity(index, devices, name).ok().as_ref() != Some(&first) {
        return None;
    }
    Some((first, id, boot_id(boot)?))
}

fn capture(
    index: &Path,
    devices: &Path,
    boot: &Path,
    name: &str,
) -> Result<(String, Option<(InterfaceIdentity, u32, String)>), String> {
    let before = sample_key(index, devices, boot, name);
    let observation = report(index, devices, name)?;
    let after = sample_key(index, devices, boot, name);
    Ok((observation, if before == after { after } else { None }))
}

pub(crate) fn run(name: &str, sample_ms: Option<u64>) -> Result<(), String> {
    if !interface_name(name) {
        return Err("expected a Linux interface name of 1 to 15 safe characters".into());
    }
    let paths = (
        Path::new("/sys/class/net"),
        Path::new("/sys/devices"),
        Path::new("/proc/sys/kernel/random/boot_id"),
    );
    let Some(wait) = sample_ms else {
        println!("{}", report(paths.0, paths.1, name)?);
        return Ok(());
    };
    if !(1..=MAX_SAMPLE_MS).contains(&wait) {
        return Err("sample-ms must be between 1 and 60000".into());
    }
    let start = std::time::Instant::now();
    let (a, first) = capture(paths.0, paths.1, paths.2, name)?;
    std::thread::sleep(std::time::Duration::from_millis(wait));
    let elapsed = start.elapsed().as_millis();
    let (b, second) = capture(paths.0, paths.1, paths.2, name)?;
    let continuity = match (first, second) {
        (Some(k), Some(j)) if k == j => "same_identity_observed",
        (Some(_), Some(_)) => "identity_changed",
        _ => "unknown",
    };
    println!(
        r#"{{"schema_version":1,"mode":"two_passive_snapshots","requested_wait_ms":{wait},"sample_start_interval_ms":{elapsed},"continuity":"{continuity}","before":{a},"after":{b}}}"#
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
        let boot = root.join("boot");
        fs::write(&boot, "11111111-1111-4111-8111-111111111111\n").unwrap();
        assert!(sample_key(&index, &devices, &boot, "eth0").is_none());
        fs::write(link.join("ifindex"), "2\n").unwrap();
        let first = sample_key(&index, &devices, &boot, "eth0").unwrap();
        fs::write(link.join("ifindex"), "3\n").unwrap();
        let second = sample_key(&index, &devices, &boot, "eth0").unwrap();
        assert_ne!(first, second);
        fs::write(&boot, "malformed\n").unwrap();
        assert!(sample_key(&index, &devices, &boot, "eth0").is_none());
        symlink(&link, index.join("eth1")).unwrap();
        assert!(report(&index, &devices, "eth1")
            .unwrap()
            .contains(r#""status":"invalid""#));
        fs::remove_dir_all(root).unwrap();
    }
}

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

fn read_field<T>(
    path: &Path,
    parse: fn(&str) -> Result<T, &'static str>,
) -> Result<T, &'static str> {
    let mut value = String::new();
    open_attribute(path)
        .map_err(reason)?
        .take(65)
        .read_to_string(&mut value)
        .map_err(reason)?;
    if value.len() > 64 {
        return Err("invalid");
    }
    parse(value.trim())
}

fn field_json<T>(result: Result<T, &'static str>, encode: impl FnOnce(T) -> String) -> String {
    match result {
        Ok(v) => format!(r#"{{"status":"observed","value":{}}}"#, encode(v)),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

fn read(path: &Path, format: fn(&str) -> Result<String, &'static str>) -> String {
    field_json(read_field(path, format), |s| s)
}

fn unsigned(s: &str) -> Result<u64, &'static str> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err("invalid");
    }
    s.parse().map_err(|_| "invalid")
}

fn decimal(s: &str) -> Result<String, &'static str> {
    unsigned(s).map(number_json)
}

fn number_json(v: u64) -> String {
    json_string(&v.to_string())
}

const COUNTER_KEYS: [&str; 9] = [
    "rx_bytes",
    "tx_bytes",
    "rx_errors",
    "tx_errors",
    "rx_dropped",
    "tx_dropped",
    "carrier_changes",
    "rx_crc_errors",
    "rx_missed_errors",
];
type Counters = [Result<u64, &'static str>; COUNTER_KEYS.len()];

fn counter_json(values: &Counters) -> String {
    COUNTER_KEYS
        .into_iter()
        .zip(values)
        .map(|(name, value)| format!(r#""{name}":{}"#, field_json(*value, number_json)))
        .collect::<Vec<_>>()
        .join(",")
}

fn counter_deltas(before: Option<Counters>, after: Option<Counters>, same: bool) -> String {
    let (Some(before), Some(after), true) = (before, after, same) else {
        return r#"{"status":"not_comparable"}"#.to_owned();
    };
    if before
        .iter()
        .zip(&after)
        .any(|(a, b)| matches!((a,b),(Ok(a),Ok(b)) if b<a))
    {
        return r#"{"status":"counter_decreased_or_reset"}"#.to_owned();
    }
    let diffs = std::array::from_fn(|i| match (before[i], after[i]) {
        (Ok(a), Ok(b)) => b.checked_sub(a).ok_or("counter_decreased_or_reset"),
        _ => Err("endpoint_unavailable"),
    });
    let comparable = diffs.iter().filter(|field| field.is_ok()).count();
    let coverage = match comparable {
        0 => "none",
        n if n == COUNTER_KEYS.len() => "complete",
        _ => "partial",
    };
    format!(
        r#"{{"status":"observed_differences","counter_coverage":"{coverage}","comparable_counters":{comparable},"counters":{{{}}}}}"#,
        counter_json(&diffs)
    )
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

fn operstate(s: &str) -> Result<String, &'static str> {
    match s {
        "unknown" | "notpresent" | "down" | "lowerlayerdown" | "testing" | "dormant" | "up" => {
            Ok(json_string(s))
        }
        _ => Err("invalid"),
    }
}

type InterfaceIdentity = (std::path::PathBuf, u64, u64);
type StatsIdentity = Result<(u64, u64), &'static str>;

fn statistics_identity(path: &Path) -> StatsIdentity {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => Ok((m.dev(), m.ino())),
        Ok(_) => Err("invalid"),
        Err(e) => Err(reason(e)),
    }
}

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
    observe(index, devices, name).map(|(json, _)| json)
}
fn observe(index: &Path, devices: &Path, name: &str) -> Result<(String, Option<Counters>), String> {
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
        Err(status) => return Ok((withheld(status), None)),
    };
    let path = &initial.0;
    let speed = read(&path.join("speed"), speed);
    let carrier = read(&path.join("carrier"), carrier);
    let duplex = read(&path.join("duplex"), duplex);
    let operstate = read(&path.join("operstate"), operstate);
    let stats = path.join("statistics");
    let stats_identity = || statistics_identity(&stats);
    let previous_stats = stats_identity();
    let counters = COUNTER_KEYS.map(|key| {
        if key == "carrier_changes" {
            read_field(&path.join(key), unsigned)
        } else {
            previous_stats.and_then(|_| read_field(&stats.join(key), unsigned))
        }
    });
    if identity().ok().as_ref() != Some(&initial) || stats_identity() != previous_stats {
        return Ok((withheld("changed"), None));
    }
    Ok((
        format!(
            r#"{{"schema_version":1,"interface":{},"status":"observed","carrier":{carrier},"speed_mbps":{speed},"duplex":{duplex},"operstate":{operstate},"counters":{{{}}}}}"#,
            json_string(name),
            counter_json(&counters)
        ),
        Some(counters),
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

type CounterSampleKey = (InterfaceIdentity, u32, String, StatsIdentity);
type CounterSnapshot = (String, Option<Counters>, Option<CounterSampleKey>);

fn sample_key(index: &Path, devices: &Path, boot: &Path, name: &str) -> Option<CounterSampleKey> {
    let first = identity(index, devices, name).ok()?;
    let id = read_id(&first.0.join("ifindex"))?.parse::<u32>().ok()?;
    if id == 0 || identity(index, devices, name).ok().as_ref() != Some(&first) {
        return None;
    }
    let stats = statistics_identity(&first.0.join("statistics"));
    Some((first, id, boot_id(boot)?, stats))
}

fn capture(
    index: &Path,
    devices: &Path,
    boot: &Path,
    name: &str,
) -> Result<CounterSnapshot, String> {
    let before = sample_key(index, devices, boot, name);
    let (observation, counters) = observe(index, devices, name)?;
    let after = sample_key(index, devices, boot, name);
    Ok((
        observation,
        counters,
        if counters.is_some() && before == after {
            after
        } else {
            None
        },
    ))
}

pub(crate) fn snapshot(name: &str) -> Result<String, String> {
    report(Path::new("/sys/class/net"), Path::new("/sys/devices"), name)
}

pub(crate) fn render(name: &str, sample_ms: Option<u64>) -> Result<String, String> {
    if !interface_name(name) {
        return Err("expected a Linux interface name of 1 to 15 safe characters".into());
    }
    let paths = (
        Path::new("/sys/class/net"),
        Path::new("/sys/devices"),
        Path::new("/proc/sys/kernel/random/boot_id"),
    );
    let Some(wait) = sample_ms else {
        return snapshot(name);
    };
    if !(1..=MAX_SAMPLE_MS).contains(&wait) {
        return Err("sample-ms must be between 1 and 60000".into());
    }
    let start = std::time::Instant::now();
    let (a, ca, first) = capture(paths.0, paths.1, paths.2, name)?;
    std::thread::sleep(std::time::Duration::from_millis(wait));
    let elapsed = start.elapsed().as_millis();
    let (b, cb, second) = capture(paths.0, paths.1, paths.2, name)?;
    let continuity = match (first, second) {
        (Some(k), Some(j)) if k == j => "same_identity_observed",
        (Some(_), Some(_)) => "identity_changed",
        _ => "unknown",
    };
    let delta = counter_deltas(ca, cb, continuity == "same_identity_observed");
    Ok(format!(
        r#"{{"schema_version":1,"mode":"two_passive_snapshots","requested_wait_ms":{wait},"sample_start_interval_ms":{elapsed},"continuity":"{continuity}","before":{a},"after":{b},"delta":{delta}}}"#
    ))
}

pub(crate) fn run(name: &str, sample_ms: Option<u64>) -> Result<(), String> {
    println!("{}", render(name, sample_ms)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn counter_diffs_preserve_intervening_link_changes_and_refuse_resets() {
        let mut a = [Err("unavailable"); COUNTER_KEYS.len()];
        let mut b = a;
        a[0] = Ok(20);
        b[0] = Ok(20);
        a[6] = Ok(9);
        b[6] = Ok(11);
        a[7] = Ok(7);
        b[7] = Ok(9);
        let delta = counter_deltas(Some(a), Some(b), true);
        assert!(delta.contains(r#""carrier_changes":{"status":"observed","value":"2"}"#));
        assert!(delta.contains(r#""rx_crc_errors":{"status":"observed","value":"2"}"#));
        assert!(delta.contains(r#""rx_missed_errors":{"status":"endpoint_unavailable"}"#));
        assert!(delta.contains(r#""rx_bytes":{"status":"observed","value":"0"}"#));
        assert!(delta.contains(r#""tx_bytes":{"status":"endpoint_unavailable"}"#));
        assert!(delta.contains(r#""counter_coverage":"partial","comparable_counters":3"#));
        assert_eq!(carrier("1"), Ok("true".into()));
        b[0] = Ok(5);
        assert_eq!(
            counter_deltas(Some(a), Some(b), true),
            r#"{"status":"counter_decreased_or_reset"}"#
        );
        assert_eq!(
            counter_deltas(Some(a), Some(b), false),
            r#"{"status":"not_comparable"}"#
        );
    }

    #[test]
    fn counter_coverage_distinguishes_no_data_from_measured_zero() {
        let none = [Err("unavailable"); COUNTER_KEYS.len()];
        let zero = [Ok(0); COUNTER_KEYS.len()];
        let empty = counter_deltas(Some(none), Some(none), true);
        assert!(empty.contains(r#""counter_coverage":"none","comparable_counters":0"#));
        assert!(!empty.contains(r#""value":"0""#));
        let all = counter_deltas(Some(zero), Some(zero), true);
        assert!(all.contains(r#""counter_coverage":"complete","comparable_counters":9"#));
        assert!(all.contains(r#""rx_errors":{"status":"observed","value":"0"}"#));
    }

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
        fs::write(link.join("operstate"), "lowerlayerdown\n").unwrap();
        fs::write(link.join("statistics/rx_crc_errors"), "21\n").unwrap();
        fs::write(link.join("statistics/rx_missed_errors"), "0\n").unwrap();
        fs::write(link.join("statistics/rx_errors"), "0\n").unwrap();
        fs::write(link.join("statistics/tx_errors"), "18446744073709551615\n").unwrap();
        let result = report(&index, &devices, "eth0").unwrap();
        assert!(result.contains(
            r#""carrier":{"status":"observed","value":false},"speed_mbps":{"status":"unavailable"}"#
        ));
        assert!(result.contains(r#""rx_errors":{"status":"observed","value":"0"},"tx_errors":{"status":"observed","value":"18446744073709551615"}"#));
        assert!(result.contains(r#""operstate":{"status":"observed","value":"lowerlayerdown"}"#));
        assert!(result.contains(r#""rx_crc_errors":{"status":"observed","value":"21"}"#));
        assert!(result.contains(r#""rx_missed_errors":{"status":"observed","value":"0"}"#));
        assert_eq!(operstate("unknown"), Ok(json_string("unknown")));
        assert_eq!(operstate("whatever"), Err("invalid"));
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
        // Keep the old inode alive while creating an alternate statistics directory.
        fs::rename(link.join("statistics"), link.join("saved-statistics")).unwrap();
        fs::create_dir(link.join("statistics")).unwrap();
        let replaced_stats = sample_key(&index, &devices, &boot, "eth0").unwrap();
        assert_ne!(second, replaced_stats);
        fs::write(&boot, "malformed\n").unwrap();
        assert!(sample_key(&index, &devices, &boot, "eth0").is_none());
        symlink(&link, index.join("eth1")).unwrap();
        assert!(report(&index, &devices, "eth1")
            .unwrap()
            .contains(r#""status":"invalid""#));
        fs::remove_dir_all(root).unwrap();
    }
}

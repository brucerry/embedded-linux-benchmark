use crate::{pcie, sysfs::open_attribute};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const LIMIT: usize = 8192;
pub(crate) const MAX_SAMPLE_MS: u64 = 60_000;
const ROOT: &str = "/sys/devices";
const COUNTERS: [(&str, &str); 3] = [
    ("aer_dev_correctable", "TOTAL_ERR_COR"),
    ("aer_dev_nonfatal", "TOTAL_ERR_NONFATAL"),
    ("aer_dev_fatal", "TOTAL_ERR_FATAL"),
];
const ROOT_COUNTERS: [&str; 3] = [
    "aer_rootport_total_err_cor",
    "aer_rootport_total_err_nonfatal",
    "aer_rootport_total_err_fatal",
];

#[derive(Debug, PartialEq, Eq)]
enum Observed<T> {
    Value(T),
    Unavailable,
    Inaccessible,
    Invalid,
}

fn read<T>(path: &Path, parse: impl FnOnce(&str) -> Option<T>) -> Observed<T> {
    let failed = |e: io::Error| match e.kind() {
        io::ErrorKind::NotFound => Observed::Unavailable,
        io::ErrorKind::PermissionDenied => Observed::Inaccessible,
        _ => Observed::Invalid,
    };
    let file = match open_attribute(path) {
        Ok(file) => file,
        Err(error) => return failed(error),
    };
    let mut text = String::new();
    if let Err(error) = file.take((LIMIT + 1) as u64).read_to_string(&mut text) {
        return failed(error);
    }
    if text.len() > LIMIT {
        return Observed::Invalid;
    }
    parse(&text).map_or(Observed::Invalid, Observed::Value)
}

fn counter(s: &str) -> Option<u64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

#[derive(Debug, PartialEq, Eq)]
struct Counts {
    total_messages: u64,
    categories: BTreeMap<String, u64>,
}

fn parse_counts(text: &str, total_key: &str) -> Option<Counts> {
    let mut total = None;
    let mut rows = BTreeMap::new();
    for (i, line) in text.lines().enumerate() {
        if i == 64 {
            return None;
        }
        let mut fields = line.split_ascii_whitespace();
        let name = fields.next()?;
        if name.len() > 64 || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        let value = counter(fields.next()?)?;
        if fields.next().is_some() {
            return None;
        }
        if name == total_key {
            if total.replace(value).is_some() {
                return None;
            }
        } else if name.starts_with("TOTAL_ERR_") || rows.insert(name.to_owned(), value).is_some() {
            return None;
        }
    }
    Some(Counts {
        total_messages: total?,
        categories: rows,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Device {
    bdf: String,
    path: PathBuf,
}

fn topology(path: &Path, sys_devices: &Path, requested: &str) -> Option<Vec<Device>> {
    let relative = path.strip_prefix(sys_devices).ok()?;
    if path.file_name()?.to_str()? != requested || relative.components().count() > 64 {
        return None;
    }
    let mut devices = Vec::new();
    for ancestor in path.ancestors().take_while(|p| *p != sys_devices) {
        if let Ok(bdf) = pcie::bdf(ancestor.file_name()?.to_str()?) {
            devices.push(Device {
                bdf,
                path: ancestor.to_path_buf(),
            });
        }
    }
    (!devices.is_empty()).then_some(devices)
}

// The root's hierarchy totals are a separate series, not sums of device counters.
fn root_count(path: &Path, name: &str) -> Observed<u64> {
    let direct = read(&path.join(name), |s| counter(s.trim()));
    if direct == Observed::Unavailable {
        match fs::symlink_metadata(path.join("aer_stats")) {
            Ok(meta) if meta.file_type().is_dir() => {
                read(&path.join("aer_stats").join(name), |s| counter(s.trim()))
            }
            Ok(_) => Observed::Invalid,
            Err(e) if e.kind() == io::ErrorKind::NotFound => Observed::Unavailable,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Observed::Inaccessible,
            Err(_) => Observed::Invalid,
        }
    } else {
        direct
    }
}

pub(crate) fn json_string(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_observed<T>(value: &Observed<T>, encode: impl FnOnce(&T) -> String) -> String {
    match value {
        Observed::Value(v) => format!(r#"{{"status":"observed","value":{}}}"#, encode(v)),
        Observed::Unavailable => r#"{"status":"unavailable"}"#.to_owned(),
        Observed::Inaccessible => r#"{"status":"inaccessible"}"#.to_owned(),
        Observed::Invalid => r#"{"status":"invalid"}"#.to_owned(),
    }
}

fn counts_json(counts: &Counts) -> String {
    let categories = counts
        .categories
        .iter()
        .map(|(name, value)| format!("{}:\"{}\"", json_string(name), value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"total_messages":"{}","categories":{{{}}}}}"#,
        counts.total_messages, categories
    )
}

#[derive(Debug, PartialEq, Eq)]
struct Identity {
    fs_device: u64,
    inode: u64,
}

fn identity(path: &Path) -> io::Result<Identity> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected a PCI sysfs directory",
        ));
    }
    Ok(Identity {
        fs_device: meta.dev(),
        inode: meta.ino(),
    })
}

struct SampleDevice {
    device: Device,
    identity: Identity,
    links: [Observed<String>; 4],
    counters: [Observed<Counts>; 3],
    root: [Observed<u64>; 3],
}

fn snapshot(devices: &[Device]) -> Result<Vec<SampleDevice>, String> {
    devices
        .iter()
        .map(|device| {
            let identify = || {
                identity(&device.path)
                    .map_err(|e| format!("cannot identify PCI device {}: {e}", device.bdf))
            };
            let before = identify()?;
            let links = pcie::LINK_ATTRIBUTES.map(|(attr, _)| {
                read(&device.path.join(attr), |s| {
                    let v = s.trim();
                    (!v.is_empty()
                        && v.len() <= 128
                        && v.bytes().all(|b| b.is_ascii_graphic() || b == b' '))
                    .then(|| v.to_owned())
                })
            });
            let counters =
                COUNTERS.map(|(name, key)| read(&device.path.join(name), |v| parse_counts(v, key)));
            let root = ROOT_COUNTERS.map(|name| root_count(&device.path, name));
            if before != identify()? {
                return Err(format!(
                    "PCI device {} changed during observation",
                    device.bdf
                ));
            }
            Ok(SampleDevice {
                device: device.clone(),
                identity: before,
                links,
                counters,
                root,
            })
        })
        .collect()
}

fn fields<V, const N: usize>(
    names: [&str; N],
    values: &[V; N],
    encode: impl Fn(&V) -> String,
) -> String {
    names
        .into_iter()
        .zip(values)
        .map(|(name, value)| format!("{}:{}", json_string(name), encode(value)))
        .collect::<Vec<_>>()
        .join(",")
}

#[derive(Clone, Copy, Default)]
pub(crate) struct LinkExpectation {
    min_width: Option<u64>,
    min_speed_tenths: Option<u64>,
}

impl LinkExpectation {
    pub(crate) fn parse(min_width: Option<u64>, min_speed: Option<&str>) -> Result<Self, String> {
        let min_speed_tenths = min_speed.map(|v| {
            speed_tenths(v).ok_or("'--min-speed-gtps' must be a decimal from 0.1 to 256.0 with at most one fractional digit")
        }).transpose()?;
        Ok(Self {
            min_width,
            min_speed_tenths,
        })
    }

    fn render(self, target: &SampleDevice) -> Option<String> {
        let width = self.min_width.map(|minimum| {
            let sample = interpret(&target.links[1], |v| counter(v).filter(|w| *w <= 32));
            format!(
                r#""width_lanes":{}"#,
                assess(minimum, sample, |s| s.to_string())
            )
        });
        let speed = self.min_speed_tenths.map(|minimum| {
            let sample = interpret(&target.links[0], link_speed);
            format!(
                r#""speed_gtps":{}"#,
                assess(minimum, sample, |s| format!("{}.{}", s / 10, s % 10))
            )
        });
        let parts = width.into_iter().chain(speed).collect::<Vec<_>>().join(",");
        (!parts.is_empty()).then(|| {
            format!(
                r#","target_link_expectation":{{"bdf":"{}",{parts}}}"#,
                target.device.bdf
            )
        })
    }
}

fn speed_tenths(v: &str) -> Option<u64> {
    let (whole, fraction) = v.split_once('.').unwrap_or((v, "0"));
    if whole.is_empty() || whole.len() > 3 || fraction.len() != 1 {
        return None;
    }
    let value = counter(whole)?
        .checked_mul(10)?
        .checked_add(counter(fraction)?)?;
    (1..=2560).contains(&value).then_some(value)
}

fn link_speed(value: &str) -> Option<u64> {
    let mut parts = value.split_ascii_whitespace();
    let speed = speed_tenths(parts.next()?)?;
    if parts.next()? != "GT/s" {
        return None;
    }
    match (parts.next(), parts.next()) {
        (None, None) | (Some("PCIe"), None) => Some(speed),
        _ => None,
    }
}

fn interpret<T>(value: &Observed<String>, parse: impl FnOnce(&str) -> Option<T>) -> Observed<T> {
    match value {
        Observed::Value(raw) => parse(raw).map_or(Observed::Invalid, Observed::Value),
        Observed::Unavailable => Observed::Unavailable,
        Observed::Inaccessible => Observed::Inaccessible,
        Observed::Invalid => Observed::Invalid,
    }
}

fn assess(minimum: u64, value: Observed<u64>, encode: impl Fn(u64) -> String) -> String {
    let expected = encode(minimum);
    let status = match value {
        Observed::Value(v) if v >= minimum => "meets_minimum",
        Observed::Value(_) => "below_minimum",
        Observed::Unavailable => "unavailable",
        Observed::Inaccessible => "inaccessible",
        Observed::Invalid => "invalid",
    };
    let observed = match value {
        Observed::Value(v) => encode(v),
        _ => "null".to_owned(),
    };
    format!(r#"{{"status":"{status}","expected_minimum":{expected},"negotiated":{observed}}}"#)
}

fn render(devices: &[SampleDevice], expectation: LinkExpectation) -> String {
    let observations = devices
        .iter()
        .enumerate()
        .map(|(i, sample)| {
            let links = fields(
                pcie::LINK_ATTRIBUTES.map(|(key, _)| key),
                &sample.links,
                |value| json_observed(value, |s| json_string(s)),
            );
            let counters = fields(COUNTERS.map(|(key, _)| key), &sample.counters, |value| {
                json_observed(value, counts_json)
            });
            format!(
                r#"{{"bdf":"{}","role":"{}","link":{{{}}},"device_aer":{{{}}}}}"#,
                sample.device.bdf,
                if i == 0 { "target" } else { "upstream" },
                links,
                counters
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let root_rows = devices
        .iter()
        .filter_map(|sample| {
            if sample
                .root
                .iter()
                .all(|value| *value == Observed::Unavailable)
            {
                return None;
            }
            let counters = fields(ROOT_COUNTERS, &sample.root, |value| {
                json_observed(value, |v| format!("\"{v}\""))
            });
            Some(format!(
                r#"{{"bdf":"{}","counters":{{{}}}}}"#,
                sample.device.bdf, counters
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    let target_link_expectation = expectation.render(&devices[0]).unwrap_or_default();
    format!(
        r#"{{"schema_version":1,"device":"{}","observations":[{}],"root_hierarchy_aer":[{}]{target_link_expectation}}}"#,
        devices[0].device.bdf, observations, root_rows
    )
}

fn uuid(text: &str) -> Option<String> {
    let text = text.trim();
    (text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        }))
    .then(|| text.to_ascii_lowercase())
}

fn read_boot() -> Observed<String> {
    read(Path::new("/proc/sys/kernel/random/boot_id"), uuid)
}

fn delta_counts(before: &Counts, after: &Counts) -> Result<Counts, &'static str> {
    if before.categories.keys().ne(after.categories.keys()) {
        return Err("category_set_changed");
    }
    let total_messages = after
        .total_messages
        .checked_sub(before.total_messages)
        .ok_or("counter_decreased")?;
    let mut categories = BTreeMap::new();
    for (name, old_value) in &before.categories {
        let delta = after.categories[name]
            .checked_sub(*old_value)
            .ok_or("counter_decreased")?;
        categories.insert(name.clone(), delta);
    }
    Ok(Counts {
        total_messages,
        categories,
    })
}

fn delta_field<T, U>(
    before: &Observed<T>,
    after: &Observed<T>,
    delta: impl FnOnce(&T, &T) -> Result<U, &'static str>,
    encode: impl FnOnce(U) -> String,
) -> String {
    let error = |reason| format!(r#"{{"status":"not_comparable","reason":"{reason}"}}"#);
    match (before, after) {
        (Observed::Value(a), Observed::Value(b)) => match delta(a, b) {
            Ok(v) => format!(r#"{{"status":"observed","value":{}}}"#, encode(v)),
            Err(e) => error(e),
        },
        (Observed::Unavailable, Observed::Unavailable) => r#"{"status":"unavailable"}"#.to_owned(),
        (Observed::Inaccessible, Observed::Inaccessible) => {
            r#"{"status":"inaccessible"}"#.to_owned()
        }
        (Observed::Invalid, Observed::Invalid) => r#"{"status":"invalid"}"#.to_owned(),
        _ => error("observation_state_changed"),
    }
}

fn comparison(
    first: &[SampleDevice],
    second: &[SampleDevice],
    boot_before: &Observed<String>,
    boot_after: &Observed<String>,
) -> Result<(), &'static str> {
    match (boot_before, boot_after) {
        (Observed::Value(a), Observed::Value(b)) if a != b => return Err("boot_changed"),
        (Observed::Value(_), Observed::Value(_)) => {}
        _ => return Err("boot_identity_unavailable"),
    }
    if first.len() != second.len()
        || first
            .iter()
            .zip(second)
            .any(|(a, b)| a.device != b.device || a.identity != b.identity)
    {
        return Err("device_identity_changed");
    }
    Ok(())
}

fn render_delta(
    first: &[SampleDevice],
    second: &[SampleDevice],
    boot_before: &Observed<String>,
    boot_after: &Observed<String>,
) -> String {
    if let Err(reason) = comparison(first, second, boot_before, boot_after) {
        return format!(r#"{{"status":"not_comparable","reason":"{reason}"}}"#);
    }
    let rows = first.iter().zip(second).map(|(a,b)| {
        let device_deltas = COUNTERS.iter().zip(a.counters.iter().zip(&b.counters))
            .map(|((name,_),(before,after))| format!("{}:{}",json_string(name),
                delta_field(before,after,delta_counts,|v| counts_json(&v))))
            .collect::<Vec<_>>().join(",");
        let root_deltas = ROOT_COUNTERS.iter().zip(a.root.iter().zip(&b.root))
            .map(|(name,(before,after))| format!("{}:{}",json_string(name),
                delta_field(before,after,|a,b| b.checked_sub(*a).ok_or("counter_decreased"),
                    |v| format!("\"{v}\""))))
            .collect::<Vec<_>>().join(",");
        format!(r#"{{"bdf":"{}","device_aer":{{{device_deltas}}},"root_hierarchy_aer":{{{root_deltas}}}}}"#,b.device.bdf)
    }).collect::<Vec<_>>().join(",");
    format!(r#"{{"status":"compared","by_observer":[{rows}]}}"#)
}

fn observe(requested: &str) -> Result<Vec<SampleDevice>, String> {
    let link = Path::new("/sys/bus/pci/devices").join(requested);
    let path = fs::canonicalize(&link)
        .map_err(|e| format!("cannot resolve PCI device {requested}: {e}"))?;
    let devices =
        topology(&path, Path::new(ROOT), requested).ok_or("unexpected PCI sysfs topology")?;
    let result = snapshot(&devices)?;
    if fs::canonicalize(&link).ok().as_ref() != Some(&path) {
        return Err("PCI device topology changed during observation".into());
    }
    Ok(result)
}

pub(crate) fn run(
    device: &str,
    sample_ms: Option<u64>,
    expectation: LinkExpectation,
) -> Result<(), String> {
    let requested = pcie::bdf(device)?;
    if let Some(ms) = sample_ms {
        let boot_before = read_boot();
        let started = Instant::now();
        let first = observe(&requested)?;
        std::thread::sleep(Duration::from_millis(ms));
        let second_started = Instant::now();
        let second = observe(&requested)?;
        let boot_after = read_boot();
        let deltas = render_delta(&first, &second, &boot_before, &boot_after);
        let interval = second_started.duration_since(started).as_millis();
        println!(
            r#"{{"schema_version":1,"device":"{requested}","requested_wait_ms":{ms},"sample_start_interval_ms":{interval},"before":{},"after":{},"delta":{deltas}}}"#,
            render(&first, expectation),
            render(&second, expectation)
        );
    } else {
        println!("{}", render(&observe(&requested)?, expectation));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_totals_are_not_computed_from_categories() {
        let result = parse_counts("RxErr 7\nBadTLP 2\nTOTAL_ERR_COR 4\n", "TOTAL_ERR_COR").unwrap();
        assert_eq!(result.total_messages, 4);
        assert_eq!(result.categories["RxErr"], 7);
        assert_eq!(
            parse_counts("TOTAL_ERR_COR 0\n", "TOTAL_ERR_COR")
                .unwrap()
                .total_messages,
            0
        );
        assert!(parse_counts("TOTAL_ERR_COR 18446744073709551615\n", "TOTAL_ERR_COR").is_some());
    }

    #[test]
    fn rejects_malformed_or_ambiguous_counter_output() {
        for input in [
            "",
            "RxErr 0",
            "TOTAL_ERR_COR +1",
            "TOTAL_ERR_COR 18446744073709551616",
            "TOTAL_ERR_COR 1\nTOTAL_ERR_COR 2",
            "TOTAL_ERR_COR 1\nRxErr 1\nRxErr 2",
            "TOTAL_ERR_COR 1 trailing",
            "TOTAL_ERR_FATAL 0\nTOTAL_ERR_COR 0",
            "TOTAL_ERR_COR 1\ninvalid: 0",
            "TOTAL_ERR_COR -1",
        ] {
            assert!(parse_counts(input, "TOTAL_ERR_COR").is_none(), "{input:?}");
        }
    }

    #[test]
    fn renders_upstream_errors_without_reassigning_them_to_the_target() {
        let root = std::env::temp_dir().join(format!("eld-pci-health-{}", std::process::id()));
        let bridge = root.join("pci0000:00/0000:00:01.0");
        let target = bridge.join("0000:01:00.0");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("aer_dev_correctable"), "TOTAL_ERR_COR 0\n").unwrap();
        fs::write(
            bridge.join("aer_dev_correctable"),
            "RxErr 7\nTOTAL_ERR_COR 4\n",
        )
        .unwrap();
        fs::create_dir(bridge.join("aer_stats")).unwrap();
        fs::write(bridge.join("aer_stats/aer_rootport_total_err_cor"), "5\n").unwrap();
        fs::write(bridge.join("aer_rootport_total_err_nonfatal"), "0\n").unwrap();
        fs::write(target.join("aer_dev_fatal"), "x".repeat(LIMIT + 1)).unwrap();
        let captured = render(
            &snapshot(&topology(&target, &root, "0000:01:00.0").unwrap()).unwrap(),
            LinkExpectation::default(),
        );
        fs::remove_dir_all(&root).unwrap();
        assert!(captured.contains("\"bdf\":\"0000:01:00.0\",\"role\":\"target\""));
        assert!(captured.contains("\"bdf\":\"0000:00:01.0\",\"role\":\"upstream\""));
        assert!(captured.contains("\"total_messages\":\"0\""));
        assert!(captured.contains("\"total_messages\":\"4\",\"categories\":{\"RxErr\":\"7\"}"));
        assert!(captured.contains("\"aer_dev_fatal\":{\"status\":\"invalid\"}"));
        assert!(captured.contains("\"root_hierarchy_aer\":[{\"bdf\":\"0000:00:01.0\""));
        assert!(captured
            .contains("\"aer_rootport_total_err_cor\":{\"status\":\"observed\",\"value\":\"5\"}"));
        assert!(captured.contains(
            "\"aer_rootport_total_err_nonfatal\":{\"status\":\"observed\",\"value\":\"0\"}"
        ));
        assert!(captured.contains("\"aer_rootport_total_err_fatal\":{\"status\":\"unavailable\"}"));
    }

    #[test]
    fn invalidates_resets_wraps_and_counter_category_changes() {
        let a = |n| parse_counts(n, "TOTAL_ERR_COR").unwrap();
        let same = a("RxErr 0\nTOTAL_ERR_COR 0");
        let more = a("RxErr 7\nTOTAL_ERR_COR 4");
        let reset = a("RxErr 6\nTOTAL_ERR_COR 9");
        let changed = a("RxErr 7\nBadTLP 1\nTOTAL_ERR_COR 4");
        assert_eq!(delta_counts(&same, &more).unwrap(), more);
        assert_eq!(delta_counts(&more, &same), Err("counter_decreased"));
        assert_eq!(delta_counts(&more, &reset), Err("counter_decreased"));
        assert_eq!(delta_counts(&more, &changed), Err("category_set_changed"));
        assert_eq!(
            delta_field(
                &Observed::Value(more),
                &Observed::Unavailable,
                delta_counts,
                |v| counts_json(&v)
            ),
            r#"{"status":"not_comparable","reason":"observation_state_changed"}"#
        );
    }

    #[test]
    fn does_not_compare_across_boots_missing_identity_or_replaced_observer() {
        let p = PathBuf::from("/sys/devices/pci0000:00/0000:00:01.0");
        let make = |inode| SampleDevice {
            device: Device {
                bdf: "0000:00:01.0".into(),
                path: p.clone(),
            },
            identity: Identity {
                fs_device: 1,
                inode,
            },
            links: std::array::from_fn(|_| Observed::Unavailable),
            counters: std::array::from_fn(|_| Observed::Unavailable),
            root: std::array::from_fn(|_| Observed::Unavailable),
        };
        let before = [make(4)];
        let same = [make(4)];
        let replacement = [make(5)];
        let a = Observed::Value(uuid("32f91701-57d4-4598-86a7-b5f27c04e425").unwrap());
        let b = Observed::Value(uuid("32f91701-57d4-4598-86a7-b5f27c04e426").unwrap());
        assert_eq!(comparison(&before, &same, &a, &a), Ok(()));
        assert_eq!(comparison(&before, &same, &a, &b), Err("boot_changed"));
        assert_eq!(
            comparison(&before, &same, &a, &Observed::Unavailable),
            Err("boot_identity_unavailable")
        );
        assert_eq!(
            comparison(&before, &replacement, &a, &a),
            Err("device_identity_changed")
        );
        assert!(uuid("not-a-real-boot-id").is_none());
        assert!(!render_delta(&before, &replacement, &a, &a).contains("by_observer"));
    }

    #[test]
    fn parses_link_speed_without_floating_point_or_ambiguous_units() {
        for (text, value) in [
            ("2.5 GT/s PCIe", 25),
            ("16.0 GT/s PCIe", 160),
            ("8 GT/s", 80),
        ] {
            assert_eq!(link_speed(text), Some(value));
        }
        for malformed in [
            "0",
            "Unknown",
            "8.00 GT/s",
            "8 Gbps",
            "8 GT/s trailing",
            "NaN",
            "-8",
            "1e2",
        ] {
            assert!(link_speed(malformed).is_none(), "{malformed}");
        }
        assert!(LinkExpectation::parse(None, Some("8.00")).is_err());
        assert!(LinkExpectation::parse(None, Some("257")).is_err());
    }

    #[test]
    fn assessment_distinguishes_shortfall_from_unavailable_or_equal() {
        let observation = |v: u64| v.to_string();
        assert_eq!(
            assess(8, Observed::Value(4), observation),
            r#"{"status":"below_minimum","expected_minimum":8,"negotiated":4}"#
        );
        assert!(assess(8, Observed::Value(8), observation).contains("meets_minimum"));
        assert!(assess(8, Observed::Unavailable, observation)
            .contains(r#""status":"unavailable","expected_minimum":8,"negotiated":null"#));
        assert!(assess(8, Observed::Invalid, observation).contains(r#""status":"invalid""#));
        assert_eq!(
            interpret(&Observed::Inaccessible, link_speed),
            Observed::Inaccessible
        );
    }

    #[test]
    fn topology_is_bounded_and_keeps_distinct_bdfs() {
        let root = Path::new("/sys/devices");
        let p = root.join("pci0000:00/0000:00:01.0/0000:01:00.0");
        let entries = topology(&p, root, "0000:01:00.0").unwrap();
        assert_eq!(
            entries.iter().map(|v| v.bdf.as_str()).collect::<Vec<_>>(),
            ["0000:01:00.0", "0000:00:01.0"]
        );
        assert!(topology(&p, root, "0000:00:01.0").is_none());
        assert!(topology(Path::new("/tmp/0000:01:00.0"), root, "0000:01:00.0").is_none());
    }
}

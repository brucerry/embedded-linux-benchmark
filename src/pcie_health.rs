use crate::{pcie, sysfs::open_attribute};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const LIMIT: usize = 8192;
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

#[derive(Debug, PartialEq, Eq)]
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

fn json_string(s: &str) -> String {
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

fn json_observed<T>(value: Observed<T>, encode: impl FnOnce(T) -> String) -> String {
    match value {
        Observed::Value(v) => format!(r#"{{"status":"observed","value":{}}}"#, encode(v)),
        Observed::Unavailable => r#"{"status":"unavailable"}"#.to_owned(),
        Observed::Inaccessible => r#"{"status":"inaccessible"}"#.to_owned(),
        Observed::Invalid => r#"{"status":"invalid"}"#.to_owned(),
    }
}

fn counts_json(counts: Counts) -> String {
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

fn render(devices: &[Device]) -> String {
    let observations = devices
        .iter()
        .enumerate()
        .map(|(i, device)| {
            let links = pcie::LINK_ATTRIBUTES
                .iter()
                .map(|(attr, _)| {
                    let value = read(&device.path.join(attr), |s| {
                        let v = s.trim();
                        (!v.is_empty()
                            && v.len() <= 128
                            && v.bytes().all(|b| b.is_ascii_graphic() || b == b' '))
                        .then(|| v.to_owned())
                    });
                    format!(
                        "{}:{}",
                        json_string(attr),
                        json_observed(value, |v| json_string(&v))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let counters = COUNTERS
                .iter()
                .map(|(name, key)| {
                    format!(
                        "{}:{}",
                        json_string(name),
                        json_observed(
                            read(&device.path.join(name), |v| parse_counts(v, key)),
                            counts_json
                        )
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                r#"{{"bdf":"{}","role":"{}","link":{{{}}},"device_aer":{{{}}}}}"#,
                device.bdf,
                if i == 0 { "target" } else { "upstream" },
                links,
                counters
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let root_rows = devices
        .iter()
        .filter_map(|device| {
            let counts = ROOT_COUNTERS.map(|name| root_count(&device.path, name));
            if counts.iter().all(|value| *value == Observed::Unavailable) {
                return None;
            }
            let counters = ROOT_COUNTERS
                .into_iter()
                .zip(counts)
                .map(|(name, value)| {
                    format!(
                        "{}:{}",
                        json_string(name),
                        json_observed(value, |v| format!("\"{v}\""))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            Some(format!(
                r#"{{"bdf":"{}","counters":{{{}}}}}"#,
                device.bdf, counters
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"schema_version":1,"device":"{}","observations":[{}],"root_hierarchy_aer":[{}]}}"#,
        devices[0].bdf, observations, root_rows
    )
}

pub(crate) fn run(device: &str) -> Result<(), String> {
    let requested = pcie::bdf(device)?;
    let link = Path::new("/sys/bus/pci/devices").join(&requested);
    let path = fs::canonicalize(&link)
        .map_err(|e| format!("cannot resolve PCI device {requested}: {e}"))?;
    let devices =
        topology(&path, Path::new(ROOT), &requested).ok_or("unexpected PCI sysfs topology")?;
    let content = render(&devices);
    if fs::canonicalize(&link).ok().as_ref() != Some(&path) {
        return Err("PCI device topology changed during observation".into());
    }
    println!("{content}");
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
        let captured = render(&topology(&target, &root, "0000:01:00.0").unwrap());
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

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

#[derive(Debug, PartialEq, Eq)]
struct RequiredPci {
    bdf: String,
    identity: Option<[u16; 2]>,
}

fn hex16(value: &str) -> Option<u16> {
    (value.len() == 4 && value.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| u16::from_str_radix(value, 16).ok())
        .flatten()
}

fn required_bdfs(list: Option<&str>) -> Result<Vec<RequiredPci>, String> {
    let Some(list) = list else {
        return Ok(Vec::new());
    };
    if list.len() > MAX_REQUIRED_PCIE * 23 {
        return Err("require-pcie accepts at most 16 complete PCI device IDs".into());
    }
    let mut out: Vec<RequiredPci> = Vec::new();
    for item in list.split(',') {
        let (address, identity) = match item.split_once('=') {
            None => (item, None),
            Some((address, pair)) => {
                let data = pair
                    .split_once(':')
                    .and_then(|(v, d)| Some([hex16(v)?, hex16(d)?]));
                (
                    address,
                    Some(data.ok_or(
                        "expected four-digit hexadecimal PCI vendor and device IDs: BDF=VVVV:DDDD",
                    )?),
                )
            }
        };
        let id = crate::pcie::bdf(address)?;
        if out.len() == MAX_REQUIRED_PCIE || out.iter().any(|p| p.bdf == id) {
            return Err("require-pcie needs at most 16 unique PCI device IDs".into());
        }
        out.push(RequiredPci { bdf: id, identity });
    }
    Ok(out)
}

type PciEntry = (std::path::PathBuf, u64, u64);

fn pci_entry(
    index: &Path,
    sys_devices: &Path,
    bdf: &str,
) -> Result<Option<PciEntry>, &'static str> {
    use std::os::unix::fs::MetadataExt;
    let index_identity = || match fs::symlink_metadata(index) {
        Ok(m) if m.is_dir() => Ok((m.dev(), m.ino())),
        Ok(_) => Err("invalid"),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => Err("inaccessible"),
        Err(_) => Err("unavailable"),
    };
    let initial_index = match index_identity() {
        Ok(id) => id,
        Err(status) => return Err(status),
    };
    let path = index.join(bdf);
    let read = || -> Result<Option<PciEntry>, &'static str> {
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
        return Err("changed");
    }
    second
}

fn pci_attr(path: &Path) -> Result<u16, &'static str> {
    let unreadable = |e: io::Error| match e.kind() {
        io::ErrorKind::NotFound => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    };
    let mut value = String::new();
    open_attribute(path)
        .map_err(unreadable)?
        .take(17)
        .read_to_string(&mut value)
        .map_err(unreadable)?;
    if value.len() > 16 {
        return Err("invalid");
    }
    value
        .trim()
        .strip_prefix("0x")
        .and_then(hex16)
        .ok_or("invalid")
}

fn assess_pci(
    index: &Path,
    devices: &Path,
    target: &RequiredPci,
) -> (&'static str, Option<[u16; 2]>) {
    let entry = pci_entry(index, devices, &target.bdf);
    let path = match &entry {
        Ok(Some((path, _, _))) => path,
        Ok(None) => return ("missing", None),
        Err(status) => return (status, None),
    };
    let Some(expected) = target.identity else {
        return ("present", None);
    };
    let read_ids = || -> Result<[u16; 2], &'static str> {
        Ok([
            pci_attr(&path.join("vendor"))?,
            pci_attr(&path.join("device"))?,
        ])
    };
    let first = read_ids();
    let second = read_ids();
    if first != second || entry != pci_entry(index, devices, &target.bdf) {
        return ("changed", None);
    }
    match second {
        Ok(actual) if actual == expected => ("match", Some(actual)),
        Ok(actual) => ("mismatch", Some(actual)),
        Err(status) => (status, None),
    }
}

fn ids_json(ids: [u16; 2]) -> String {
    format!(r#"{{"vendor":"{:04x}","device":"{:04x}"}}"#, ids[0], ids[1])
}

const DT_COMPATIBLE: &str = "/sys/firmware/devicetree/base/compatible";

struct BoardProfile {
    compatible: String,
    required: Vec<RequiredPci>,
}

fn specific_compatible(s: &str) -> bool {
    s.len() <= 128
        && s.split_once(',').is_some_and(|(vendor, board)| {
            [vendor, board].into_iter().all(|v| {
                !v.is_empty()
                    && v.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+')
                    })
            }) && !board.eq_ignore_ascii_case("generic")
        })
}

fn first_compatible(raw: &str) -> Option<&str> {
    if !raw.ends_with('\0') {
        return None;
    }
    let first = raw.split('\0').next()?;
    specific_compatible(first).then_some(first)
}

fn applicability(profile: &BoardProfile, observed: Option<&str>) -> &'static str {
    match observed {
        Some(board) if board == profile.compatible => "applicable",
        Some(_) => "not_applicable",
        None => "unknown",
    }
}

fn parse_profile(text: &str) -> Result<BoardProfile, String> {
    let mut lines = text.lines();
    if lines.next() != Some("eld-board-profile=1") {
        return Err("expected eld-board-profile=1".into());
    }
    let board = lines
        .next()
        .and_then(|row| row.strip_prefix("compatible="))
        .filter(|candidate| specific_compatible(candidate))
        .ok_or("expected a specific compatible=vendor,board field")?;
    let pci = lines
        .next()
        .and_then(|row| row.strip_prefix("require-pcie="))
        .ok_or("expected a require-pcie field")?;
    if lines.next().is_some() {
        return Err("unexpected board profile fields".into());
    }
    Ok(BoardProfile {
        compatible: board.into(),
        required: required_bdfs(Some(pci))?,
    })
}

// Linux emits a sorted list of individual IDs and inclusive ranges. Count without expansion.
fn id_count(input: &str) -> Option<u32> {
    fn id(s: &str) -> Option<u32> {
        if s.is_empty() || s.len() > 7 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n = s.parse().ok()?;
        (n < 1_000_000).then_some(n)
    }
    let mut count = 0u32;
    let mut last = None;
    for part in input.trim().split(',') {
        let (start, end) = match part.split_once('-') {
            Some((a, b)) => (id(a)?, id(b)?),
            None => {
                let n = id(part)?;
                (n, n)
            }
        };
        if end < start || last.is_some_and(|previous| start <= previous) {
            return None;
        }
        count = count.checked_add(end - start + 1)?;
        last = Some(end);
    }
    Some(count)
}

fn metric(path: &str, parse: impl FnOnce(&str) -> Option<u64>) -> Observation {
    match bounded(Path::new(path)) {
        Ok(v) => parse(&v).map_or(Observation::Invalid, |n| {
            Observation::Available(n.to_string())
        }),
        Err(e) => Observation::error(e),
    }
}

fn mem_value(input: &str, field: &str) -> Observation {
    let mut found = None;
    for row in input.lines() {
        let Some((key, raw)) = row.split_once(':') else {
            continue;
        };
        if key != field {
            continue;
        }
        if found.is_some() {
            return Observation::Invalid;
        }
        let mut tokens = raw.split_ascii_whitespace();
        let value = tokens
            .next()
            .filter(|x| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|x| x.parse::<u64>().ok())
            .and_then(|n| n.checked_mul(1024));
        if tokens.next() != Some("kB") || tokens.next().is_some() {
            return Observation::Invalid;
        }
        match value {
            Some(value) => found = Some(value),
            None => return Observation::Invalid,
        }
    }
    found.map_or(Observation::Unavailable, |n| {
        Observation::Available(n.to_string())
    })
}

fn resources() -> String {
    let cpus = metric("/sys/devices/system/cpu/online", |v| {
        id_count(v).map(u64::from)
    });
    let numa = metric("/sys/devices/system/node/online", |v| {
        id_count(v).map(u64::from)
    });
    let memory = bounded(Path::new("/proc/meminfo"));
    let mem = |field| match &memory {
        Ok(data) => mem_value(data, field),
        Err(e) => Observation::error(io::Error::from(e.kind())),
    };
    format!(
        r#"{{"online_cpu_count":{},"online_numa_node_count":{},"memory_total_bytes":{},"memory_available_bytes":{}}}"#,
        cpus.json(),
        numa.json(),
        mem("MemTotal").json(),
        mem("MemAvailable").json()
    )
}

fn numbered_child(name: &str, prefix: &str) -> Option<u32> {
    let id = name.strip_prefix(prefix)?;
    if id.is_empty() || id.len() > 7 || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = id.parse::<u32>().ok()?;
    (n.to_string() == id).then_some(n)
}

fn thermal_value(raw: &str) -> Option<String> {
    let v = raw.trim();
    let digits = v.strip_prefix('-').unwrap_or(v);
    if digits.is_empty() || digits.len() > 8 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = v.parse::<i32>().ok()?;
    (-273_150..=300_000).contains(&n).then(|| n.to_string())
}

struct ThermalRow {
    id: u32,
    key: Option<(u64, u64)>,
    kind: Observation,
    temp: Observation,
}

struct ThermalSnapshot {
    status: &'static str,
    zones: Vec<ThermalRow>,
}

impl ThermalSnapshot {
    fn json(&self) -> String {
        let rows = self
            .zones
            .iter()
            .map(|r| {
                format!(
                    r#"{{"zone":"thermal_zone{}","type":{},"temperature_millicelsius":{}}}"#,
                    r.id,
                    r.kind.json(),
                    r.temp.json()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"status":"{}","zones":[{rows}]}}"#, self.status)
    }
}

fn thermal_snapshot(index: &Path, devices: &Path) -> ThermalSnapshot {
    use std::os::unix::fs::MetadataExt;
    let result = |status, zones| ThermalSnapshot { status, zones };
    let entries = match fs::read_dir(index) {
        Ok(v) => v,
        Err(e) => {
            return result(
                match e.kind() {
                    io::ErrorKind::PermissionDenied => "inaccessible",
                    io::ErrorKind::NotFound => "unavailable",
                    _ => "invalid",
                },
                Vec::new(),
            )
        }
    };
    let mut ids = Vec::new();
    for (seen, row) in entries.enumerate() {
        if seen >= 256 {
            return result("truncated", Vec::new());
        }
        let Ok(row) = row else {
            return result("invalid", Vec::new());
        };
        if let Some(id) = row
            .file_name()
            .to_str()
            .and_then(|name| numbered_child(name, "thermal_zone"))
        {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    let status = if ids.len() > 32 {
        "truncated"
    } else {
        "observed"
    };
    let zones = ids
        .into_iter()
        .take(32)
        .map(|id| {
            let name = format!("thermal_zone{id}");
            let path = index.join(&name);
            let allowed = devices.join(&name);
            let probe = |leaf: &str, parse: fn(&str) -> Option<String>| match bounded(
                &allowed.join(leaf),
            ) {
                Ok(value) => parse(&value).map_or(Observation::Invalid, Observation::Available),
                Err(e) => Observation::error(e),
            };
            let identity = || {
                let actual = fs::canonicalize(&path).ok()?;
                let meta = fs::symlink_metadata(&actual).ok()?;
                (actual == allowed && meta.is_dir()).then(|| (meta.dev(), meta.ino()))
            };
            let held = identity();
            if held.is_some() {
                let kind = probe("type", text_value);
                let temp = probe("temp", thermal_value);
                if identity() == held {
                    return ThermalRow {
                        id,
                        key: held,
                        kind,
                        temp,
                    };
                }
            }
            ThermalRow {
                id,
                key: None,
                kind: Observation::Unavailable,
                temp: Observation::Unavailable,
            }
        })
        .collect();
    result(status, zones)
}

#[cfg(test)]
fn thermal(index: &Path, devices: &Path) -> String {
    thermal_snapshot(index, devices).json()
}

fn thermal_window(
    first: &ThermalSnapshot,
    second: &ThermalSnapshot,
    wait: u64,
    elapsed: u128,
) -> String {
    let mut ids = first
        .zones
        .iter()
        .chain(&second.zones)
        .map(|r| r.id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    let differences = ids
        .into_iter()
        .map(|id| {
            let a = first.zones.iter().find(|r| r.id == id);
            let b = second.zones.iter().find(|r| r.id == id);
            let difference = match (a, b) {
                (Some(a), Some(b)) if a.key.is_some() && a.key == b.key => {
                    match (&a.kind, &b.kind, &a.temp, &b.temp) {
                        (Observation::Available(ka), Observation::Available(kb), _, _)
                            if ka != kb =>
                        {
                            None
                        }
                        (
                            Observation::Available(_),
                            Observation::Available(_),
                            Observation::Available(va),
                            Observation::Available(vb),
                        ) => va
                            .parse::<i32>()
                            .ok()
                            .zip(vb.parse::<i32>().ok())
                            .and_then(|(v, w)| w.checked_sub(v)),
                        _ => None,
                    }
                }
                _ => None,
            };
            let encoded = difference.map_or_else(
                || r#"{"status":"not_comparable"}"#.into(),
                |v| format!(r#"{{"status":"observed_difference","value":"{v}"}}"#),
            );
            format!(r#"{{"zone":"thermal_zone{id}","difference_millicelsius":{encoded}}}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"{{"requested_wait_ms":{wait},"sample_start_interval_ms":{elapsed},"before":{},"after":{},"differences":[{differences}]}}"#,
        first.json(),
        second.json()
    )
}

fn edac(root: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let result = |status, rows: &str| format!(r#"{{"status":"{status}","controllers":[{rows}]}}"#);
    let identify = |path: &Path| {
        let m = fs::symlink_metadata(path)?;
        if !m.is_dir() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok((m.dev(), m.ino()))
    };
    fn status(e: io::Error) -> &'static str {
        match e.kind() {
            io::ErrorKind::PermissionDenied => "inaccessible",
            io::ErrorKind::NotFound => "unavailable",
            _ => "invalid",
        }
    }
    let root_identity = match identify(root) {
        Ok(id) => id,
        Err(e) => return result(status(e), ""),
    };
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(e) => return result(status(e), ""),
    };
    let mut ids = Vec::new();
    for (seen, row) in entries.enumerate() {
        if seen >= 256 {
            return result("truncated", "");
        }
        let Ok(row) = row else {
            return result("invalid", "");
        };
        if let Some(id) = row
            .file_name()
            .to_str()
            .and_then(|name| numbered_child(name, "mc"))
        {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    let overall = if ids.is_empty() {
        "no_controllers"
    } else if ids.len() > 32 {
        "truncated"
    } else {
        "observed"
    };
    let rows = ids
        .into_iter()
        .take(32)
        .map(|id| {
            let name = format!("mc{id}");
            let path = root.join(&name);
            let counter = |file| match bounded(&path.join(file)) {
                Ok(v) => {
                    let v = v.trim();
                    if !v.is_empty() && v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit()) {
                        v.parse::<u64>()
                            .ok()
                            .map(|n| Observation::Available(n.to_string()))
                            .unwrap_or(Observation::Invalid)
                    } else {
                        Observation::Invalid
                    }
                }
                Err(e) => Observation::error(e),
            };
            let [ce, ue, age] = match identify(&path) {
                Ok(initial) => {
                    let sample = ["ce_count","ue_count","seconds_since_reset"].map(counter);
                    if identify(&path).ok() == Some(initial) {
                        sample
                    } else {
                        std::array::from_fn(|_| Observation::Unavailable)
                    }
                }
                Err(e) => std::array::from_fn(|_| Observation::error(io::Error::from(e.kind()))),
            };
            format!(
                r#"{{"controller":"{name}","corrected_errors":{},"uncorrected_errors":{},"seconds_since_reset":{}}}"#,
                ce.json(), ue.json(), age.json()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    if identify(root).ok() != Some(root_identity) {
        result("changed", "")
    } else {
        result(overall, &rows)
    }
}

fn cpu_frequency(root: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let result = |status, lists: &str| format!(r#"{{"status":"{status}","policies":[{lists}]}}"#);
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(e) => {
            return result(
                match e.kind() {
                    io::ErrorKind::NotFound => "unavailable",
                    io::ErrorKind::PermissionDenied => "inaccessible",
                    _ => "invalid",
                },
                "",
            )
        }
    };
    let mut ids = Vec::new();
    for (seen, row) in entries.enumerate() {
        if seen >= 256 {
            return result("truncated", "");
        }
        let Ok(row) = row else {
            return result("invalid", "");
        };
        if let Some(id) = row
            .file_name()
            .to_str()
            .and_then(|v| numbered_child(v, "policy"))
        {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    let status = if ids.is_empty() {
        "no_policies"
    } else if ids.len() > 32 {
        "truncated"
    } else {
        "observed"
    };
    let rows=ids.into_iter().take(32).map(|id| {
        let name=format!("policy{id}");
        let path=root.join(&name);
        let before=fs::symlink_metadata(&path);
        let report=|leaf| match bounded(&path.join(leaf)) {
            Ok(v)=>{
                let v=v.trim();
                if v.len()<=20 && v.bytes().all(|b|b.is_ascii_digit()) {
                    v.parse::<u64>().ok().filter(|n|*n>0).map(|n|Observation::Available(n.to_string()))
                        .unwrap_or(Observation::Invalid)
                } else {Observation::Invalid}
            }
            Err(e)=>Observation::error(e),
        };
        let values=match before {
            Ok(m) if m.is_dir()=>{
                let cols=["scaling_cur_freq","cpuinfo_cur_freq","scaling_max_freq","cpuinfo_max_freq"].map(report);
                match fs::symlink_metadata(&path) {
                    Ok(end) if end.is_dir() && (end.dev(),end.ino())==(m.dev(),m.ino())=>cols,
                    _=>std::array::from_fn(|_|Observation::Unavailable),
                }
            }
            Ok(_)=>std::array::from_fn(|_|Observation::Invalid),
            Err(e)=>std::array::from_fn(|_|Observation::error(io::Error::from(e.kind()))),
        };
        format!(r#"{{"policy":"{name}","scaling_reported_khz":{},"cpuinfo_current_khz":{},"policy_max_khz":{},"cpuinfo_max_khz":{}}}"#,
            values[0].json(),values[1].json(),values[2].json(),values[3].json())
    }).collect::<Vec<_>>().join(",");
    result(status, &rows)
}

fn cpu_throttle(root: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let result =
        |status, rows: &str| format!(r#"{{"status":"{status}","cpu_observers":[{rows}]}}"#);
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(e) => {
            return result(
                match e.kind() {
                    io::ErrorKind::NotFound => "unavailable",
                    io::ErrorKind::PermissionDenied => "inaccessible",
                    _ => "invalid",
                },
                "",
            )
        }
    };
    let mut ids = Vec::new();
    for (seen, row) in entries.enumerate() {
        if seen >= 512 {
            return result("truncated", "");
        }
        let Ok(row) = row else {
            return result("invalid", "");
        };
        if let Some(id) = row
            .file_name()
            .to_str()
            .and_then(|v| numbered_child(v, "cpu"))
        {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    let limited = ids.len() > 64;
    let mut rows = Vec::new();
    for id in ids.into_iter().take(64) {
        let cpu = root.join(format!("cpu{id}"));
        let path = cpu.join("thermal_throttle");
        let identity = |p: &Path| -> io::Result<(u64, u64)> {
            let m = fs::symlink_metadata(p)?;
            if !m.is_dir() {
                return Err(io::ErrorKind::InvalidData.into());
            }
            Ok((m.dev(), m.ino()))
        };
        let before = match (identity(&cpu), identity(&path)) {
            (Ok(c), Ok(p)) => (c, p),
            (Ok(_), Err(e)) if e.kind() == io::ErrorKind::NotFound => continue,
            (Err(e), _) | (_, Err(e)) => {
                let observation = Observation::error(e).json();
                rows.push(format!(r#"{{"cpu":{id},"core_count":{observation},"package_count_at_cpu":{observation}}}"#));
                continue;
            }
        };
        let read = |file| match bounded(&path.join(file)) {
            Err(e) => Observation::error(e),
            Ok(v) => {
                let v = v.trim();
                if v.len() <= 20 && v.bytes().all(|b| b.is_ascii_digit()) {
                    v.parse::<u64>()
                        .ok()
                        .map(|n| Observation::Available(n.to_string()))
                        .unwrap_or(Observation::Invalid)
                } else {
                    Observation::Invalid
                }
            }
        };
        let mut core = read("core_throttle_count");
        let mut package = read("package_throttle_count");
        if (identity(&cpu).ok(), identity(&path).ok()) != (Some(before.0), Some(before.1)) {
            core = Observation::Unavailable;
            package = Observation::Unavailable;
        }
        rows.push(format!(
            r#"{{"cpu":{id},"core_count":{},"package_count_at_cpu":{}}}"#,
            core.json(),
            package.json()
        ));
    }
    let status = if limited {
        "truncated"
    } else if rows.is_empty() {
        "unavailable"
    } else {
        "observed"
    };
    result(status, &rows.join(","))
}

fn powercap_target(root: &Path, zone: &str) -> Option<std::path::PathBuf> {
    if zone.len() > 96 {
        return None;
    }
    let (driver, levels) = zone.split_once(':')?;
    if driver.is_empty()
        || !driver.as_bytes()[0].is_ascii_alphanumeric()
        || !driver
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return None;
    }
    let mut path = root.join(driver);
    let mut prefix = driver.to_owned();
    for (i, part) in levels.split(':').enumerate() {
        if i >= 4 || numbered_child(part, "").is_none() {
            return None;
        }
        prefix.push(':');
        prefix.push_str(part);
        path.push(&prefix);
    }
    Some(path)
}

fn powercap(index: &Path, root: &Path) -> String {
    use std::os::unix::fs::MetadataExt;
    let result = |status, rows: &str| format!(r#"{{"status":"{status}","zones":[{rows}]}}"#);
    let entries = match fs::read_dir(index) {
        Ok(v) => v,
        Err(e) => {
            return result(
                match e.kind() {
                    io::ErrorKind::NotFound => "unavailable",
                    io::ErrorKind::PermissionDenied => "inaccessible",
                    _ => "invalid",
                },
                "",
            )
        }
    };
    let mut candidates = Vec::new();
    for (seen, row) in entries.enumerate() {
        if seen >= 256 {
            return result("truncated", "");
        }
        let Ok(row) = row else {
            return result("invalid", "");
        };
        let file = row.file_name();
        let Some(name) = file.to_str() else {
            continue;
        };
        if let Some(target) = powercap_target(root, name) {
            candidates.push((name.to_owned(), target));
        }
    }
    candidates.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let status = if candidates.is_empty() {
        "no_zones"
    } else if candidates.len() > 32 {
        "truncated"
    } else {
        "observed"
    };
    let rows=candidates.into_iter().take(32).map(|(name,target)|{
        let linked=index.join(&name);
        let identify=||{
            let observed=fs::canonicalize(&linked).ok()?;
            let m=fs::symlink_metadata(&observed).ok()?;
            (observed==target && m.is_dir()).then(||(m.dev(),m.ino()))
        };
        let mut info=[Observation::Unavailable,Observation::Unavailable,Observation::Unavailable];
        if let Some(first)=identify() {
            info=["name","energy_uj","max_energy_range_uj"].map(|attr|match bounded(&target.join(attr)) {
                Ok(v) if attr=="name"=>text_value(&v).map_or(Observation::Invalid,Observation::Available),
                Ok(v)=>{
                    let v=v.trim();
                    if v.len()<=20 && v.bytes().all(|b|b.is_ascii_digit()) {
                        v.parse::<u64>().ok().map(|n|Observation::Available(n.to_string())).unwrap_or(Observation::Invalid)
                    } else {Observation::Invalid}
                }
                Err(e)=>Observation::error(e),
            });
            if identify()!=Some(first) {info=std::array::from_fn(|_|Observation::Unavailable);}
        }
        format!(r#"{{"zone":{},"name":{},"energy_microjoules":{},"max_energy_range_microjoules":{}}}"#,
            json_string(&name),info[0].json(),info[1].json(),info[2].json())
    }).collect::<Vec<_>>().join(",");
    result(status, &rows)
}

pub(crate) fn run(
    required: Option<&str>,
    profile_path: Option<&Path>,
    thermal_ms: Option<u64>,
) -> Result<(), String> {
    if required.is_some() && profile_path.is_some() {
        return Err("--require-pcie and --board-profile cannot be combined".into());
    }
    let mut required = required_bdfs(required)?;
    let profile_context = if let Some(path) = profile_path {
        let raw =
            bounded(path).map_err(|_| "expected a readable board profile file up to 4096 bytes")?;
        let profile = parse_profile(&raw)?;
        let raw_board = bounded(Path::new(DT_COMPATIBLE)).ok();
        let board = raw_board.as_deref().and_then(first_compatible);
        let status = applicability(&profile, board);
        let mark = format!(
            r#","board_profile":{{"version":1,"applicability":"{status}","expected_compatible":{},"observed_compatible":{}}}"#,
            json_string(&profile.compatible),
            board.map_or_else(|| "null".into(), json_string)
        );
        if status == "applicable" {
            required = profile.required;
        }
        mark
    } else {
        String::new()
    };
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
            .map(|target| {
                let (status, observed) = assess_pci(
                    Path::new("/sys/bus/pci/devices"),
                    Path::new("/sys/devices"),
                    target,
                );
                let details = target.identity.map_or(String::new(), |expected| {
                    format!(
                        r#","expected":{},"observed":{}"#,
                        ids_json(expected),
                        observed.map_or_else(|| "null".into(), ids_json)
                    )
                });
                format!(
                    r#"{{"bdf":{},"status":"{status}"{details}}}"#,
                    json_string(&target.bdf)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#","required_pcie":[{rows}]"#)
    };
    let paths = (
        Path::new("/sys/class/thermal"),
        Path::new("/sys/devices/virtual/thermal"),
    );
    let started = std::time::Instant::now();
    let temperatures = thermal_snapshot(paths.0, paths.1);
    let timed = if let Some(wait) = thermal_ms {
        if !(1..=60_000).contains(&wait) {
            return Err("thermal-sample-ms must be between 1 and 60000".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(wait));
        let elapsed = started.elapsed().as_millis();
        let after = thermal_snapshot(paths.0, paths.1);
        format!(
            r#","thermal_window":{}"#,
            thermal_window(&temperatures, &after, wait, elapsed)
        )
    } else {
        String::new()
    };
    println!(
        r#"{{"schema_version":1,"process_architecture":{},{values},"resources":{},"thermal":{},"memory_edac":{},"cpu_frequency":{},"cpu_thermal_throttle":{},"powercap":{}{profile_context}{profile}{timed}}}"#,
        json_string(std::env::consts::ARCH),
        resources(),
        temperatures.json(),
        edac(Path::new("/sys/devices/system/edac/mc")),
        cpu_frequency(Path::new("/sys/devices/system/cpu/cpufreq")),
        cpu_throttle(Path::new("/sys/devices/system/cpu")),
        powercap(
            Path::new("/sys/class/powercap"),
            Path::new("/sys/devices/virtual/powercap")
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powercap_reports_parent_and_child_without_combining_them() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!("eld-powercap-{}", std::process::id()));
        let root = dir.join("cap");
        let index = dir.join("index");
        let parent = root.join("intel-rapl/intel-rapl:0");
        let child = parent.join("intel-rapl:0:1");
        assert!(powercap(&index, &root).contains(r#""status":"unavailable""#));
        fs::create_dir_all(&child).unwrap();
        fs::create_dir(&index).unwrap();
        symlink(&parent, index.join("intel-rapl:0")).unwrap();
        symlink(&child, index.join("intel-rapl:0:1")).unwrap();
        symlink(&dir, index.join("intel-rapl:2")).unwrap();
        fs::write(parent.join("energy_uj"), "14\n").unwrap();
        fs::write(child.join("energy_uj"), "7\n").unwrap();
        fs::write(child.join("max_energy_range_uj"), "4294967296\n").unwrap();
        let o = powercap(&index, &root);
        assert!(o.contains(r#""energy_microjoules":{"status":"observed","value":"14"}"#));
        assert!(o.contains(r#""energy_microjoules":{"status":"observed","value":"7"}"#));
        assert!(o.contains(
            r#""max_energy_range_microjoules":{"status":"observed","value":"4294967296"}"#
        ));
        assert!(o.contains(r#""zone":"intel-rapl:2","name":{"status":"unavailable"}"#));
        assert!(!o.contains(r#""value":"21""#));
        assert!(powercap_target(&root, "intel-rapl:../0").is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn thermal_throttle_keeps_package_observations_separate() {
        let root = std::env::temp_dir().join(format!("eld-cputhrottle-{}", std::process::id()));
        let a = root.join("cpu0/thermal_throttle");
        let b = root.join("cpu1/thermal_throttle");
        assert!(cpu_throttle(&root).contains(r#""status":"unavailable""#));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        for p in [&a, &b] {
            fs::write(p.join("package_throttle_count"), "7\n").unwrap();
        }
        fs::write(a.join("core_throttle_count"), "0\n").unwrap();
        let out = cpu_throttle(&root);
        assert_eq!(
            out.matches(r#""package_count_at_cpu":{"status":"observed","value":"7"}"#)
                .count(),
            2
        );
        assert!(out.contains(r#""cpu":0,"core_count":{"status":"observed","value":"0"}"#));
        assert!(out.contains(r#""cpu":1,"core_count":{"status":"unavailable"}"#));
        assert!(!out.contains(r#""value":"14""#));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cpu_frequency_separates_policy_reporting_from_hardware() {
        let root = std::env::temp_dir().join(format!("eld-cpufreq-{}", std::process::id()));
        let policy = root.join("policy0");
        assert!(cpu_frequency(&root).contains(r#""status":"unavailable""#));
        fs::create_dir_all(&policy).unwrap();
        fs::write(policy.join("scaling_cur_freq"), "1200000\n").unwrap();
        fs::write(policy.join("scaling_max_freq"), "2400000\n").unwrap();
        fs::write(policy.join("cpuinfo_max_freq"), "0\n").unwrap();
        let out = cpu_frequency(&root);
        assert!(out.contains(r#""scaling_reported_khz":{"status":"observed","value":"1200000"}"#));
        assert!(out.contains(r#""cpuinfo_current_khz":{"status":"unavailable"}"#));
        assert!(out.contains(r#""policy_max_khz":{"status":"observed","value":"2400000"}"#));
        assert!(out.contains(r#""cpuinfo_max_khz":{"status":"invalid"}"#));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn edac_separates_corrected_uncorrected_and_unavailable() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!("eld-edac-{}", std::process::id()));
        let first = root.join("mc1");
        let second = root.join("mc2");
        assert!(edac(&root).contains(r#""status":"unavailable""#));
        fs::create_dir(&root).unwrap();
        assert!(edac(&root).contains(r#""status":"no_controllers""#));
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("ce_count"), "18446744073709551615\n").unwrap();
        fs::write(first.join("ue_count"), "0\n").unwrap();
        fs::write(first.join("seconds_since_reset"), "31\n").unwrap();
        let dimm = first.join("dimm0");
        fs::create_dir(&dimm).unwrap();
        fs::write(dimm.join("dimm_ce_count"), "99112233\n").unwrap();
        fs::write(first.join("ce_noinfo_count"), "88776655\n").unwrap();
        fs::write(second.join("ce_count"), "18446744073709551616\n").unwrap();
        symlink(&first, root.join("mc3")).unwrap();
        let out = edac(&root);
        assert!(out.contains(r#""controller":"mc1","corrected_errors":{"status":"observed","value":"18446744073709551615"},"uncorrected_errors":{"status":"observed","value":"0"},"seconds_since_reset":{"status":"observed","value":"31"}"#));
        assert!(out.contains(r#""controller":"mc2","corrected_errors":{"status":"invalid"},"uncorrected_errors":{"status":"unavailable"}"#));
        assert!(out.contains(r#""controller":"mc3","corrected_errors":{"status":"invalid"}"#));
        assert!(!out.contains("99112233") && !out.contains("88776655"));
        assert!(out.contains(r#""seconds_since_reset":{"status":"unavailable"}"#));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn thermal_window_withholds_differences_for_replaced_sensor() {
        fn sample(id: u32, key: (u64, u64), type_name: &str, value: &str) -> ThermalRow {
            ThermalRow {
                id,
                key: Some(key),
                kind: Observation::Available(type_name.into()),
                temp: Observation::Available(value.into()),
            }
        }
        let a = ThermalSnapshot {
            status: "observed",
            zones: vec![
                sample(0, (1, 2), "core", "31000"),
                sample(1, (1, 3), "other", "28000"),
            ],
        };
        let b = ThermalSnapshot {
            status: "observed",
            zones: vec![
                sample(0, (1, 2), "core", "30500"),
                sample(1, (1, 4), "other", "28500"),
            ],
        };
        let result = thermal_window(&a, &b, 25, 26);
        assert!(result.contains(r#""zone":"thermal_zone0","difference_millicelsius":{"status":"observed_difference","value":"-500"}"#));
        assert!(result.contains(
            r#""zone":"thermal_zone1","difference_millicelsius":{"status":"not_comparable"}"#
        ));
    }

    #[test]
    fn thermal_reads_signed_millicelsius_and_fixed_paths_only() {
        use std::os::unix::fs::symlink;
        assert_eq!(thermal_value("-2000\n"), Some("-2000".into()));
        assert_eq!(thermal_value("0"), Some("0".into()));
        assert_eq!(thermal_value("310000"), None);
        assert_eq!(thermal_value("17 C"), None);
        assert_eq!(numbered_child("thermal_zone02", "thermal_zone"), None);
        let root = std::env::temp_dir().join(format!("eld-thermal-{}", std::process::id()));
        let index = root.join("index");
        let devices = root.join("devices");
        let zone = devices.join("thermal_zone0");
        assert!(thermal(&index, &devices).contains(r#""status":"unavailable""#));
        fs::create_dir_all(&zone).unwrap();
        fs::create_dir(&index).unwrap();
        fs::write(zone.join("type"), "cpu-fixture\n").unwrap();
        fs::write(zone.join("temp"), "0\n").unwrap();
        symlink(&zone, index.join("thermal_zone0")).unwrap();
        symlink(&root, index.join("thermal_zone1")).unwrap();
        let value = thermal(&index, &devices);
        assert!(value.contains(r#""type":{"status":"observed","value":"cpu-fixture"},"temperature_millicelsius":{"status":"observed","value":"0"}"#));
        assert!(value.contains(r#""zone":"thermal_zone1","type":{"status":"unavailable"}"#));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resources_count_without_expansion_or_inventing_missing_memory() {
        assert_eq!(id_count("0-3,5,8-9\n"), Some(7));
        for bad in ["", "3-2", "0-3,3", "0,0", "0, 1", "0-999999999", "0,"] {
            assert_eq!(id_count(bad), None);
        }
        let data = "MemTotal: 16 kB\nMemAvailable: 0 kB\nOther: 7 kB";
        assert_eq!(
            mem_value(data, "MemTotal"),
            Observation::Available("16384".into())
        );
        assert_eq!(
            mem_value(data, "MemAvailable"),
            Observation::Available("0".into())
        );
        assert_eq!(mem_value(data, "Missing"), Observation::Unavailable);
        assert_eq!(
            mem_value("MemTotal: 3 mB", "MemTotal"),
            Observation::Invalid
        );
        assert_eq!(
            mem_value("MemTotal: 4 kB\nMemTotal: 5 kB", "MemTotal"),
            Observation::Invalid
        );
        assert_eq!(
            mem_value("MemTotal: 18446744073709551615 kB", "MemTotal"),
            Observation::Invalid
        );
    }

    #[test]
    fn board_profile_uses_first_compatible_and_fails_closed() {
        let p = parse_profile(
            "eld-board-profile=1\ncompatible=maker,alpha\nrequire-pcie=0000:01:00.0=8086:1234\n",
        )
        .unwrap();
        assert_eq!(
            applicability(&p, first_compatible("maker,alpha\0maker,soc\0")),
            "applicable"
        );
        assert_eq!(
            applicability(&p, first_compatible("maker,beta\0maker,soc\0")),
            "not_applicable"
        );
        assert_eq!(
            applicability(&p, first_compatible("generic\0maker,alpha\0")),
            "unknown"
        );
        assert_eq!(applicability(&p, None), "unknown");
        assert_eq!(first_compatible("maker,alpha"), None);
        assert!(parse_profile(
            "eld-board-profile=2\ncompatible=maker,alpha\nrequire-pcie=0000:01:00.0"
        )
        .is_err());
        assert!(parse_profile(
            "eld-board-profile=1\ncompatible=generic\nrequire-pcie=0000:01:00.0"
        )
        .is_err());
        assert_eq!(p.required.len(), 1);
    }

    #[test]
    fn required_pcie_checks_bounds_and_bus_availability() {
        use std::os::unix::fs::symlink;
        assert!(required_bdfs(Some("0000:0A:00.0,0000:0a:00.0")).is_err());
        assert!(required_bdfs(Some("../outside")).is_err());
        assert!(required_bdfs(Some(&"x".repeat(369))).is_err());
        assert_eq!(
            required_bdfs(Some("0000:0A:00.0")).unwrap()[0].bdf,
            "0000:0a:00.0"
        );
        let root = std::env::temp_dir().join(format!("eld-inventory-pcie-{}", std::process::id()));
        let index = root.join("bus/pci/devices");
        let devices = root.join("devices");
        let id = "0000:01:00.0";
        let path = index.join(id);
        let dst = devices.join("pci0000:00").join(id);
        let required = required_bdfs(Some(id)).unwrap().pop().unwrap();
        let presence = || assess_pci(&index, &devices, &required).0;
        let exact = required_bdfs(Some("0000:01:00.0=8086:ABCD"))
            .unwrap()
            .pop()
            .unwrap();
        let identity = || assess_pci(&index, &devices, &exact);
        assert!(required_bdfs(Some("0000:01:00.0=8086:abc")).is_err());
        assert!(required_bdfs(Some("0000:01:00.0=8086:abcd,0000:01:00.0")).is_err());
        assert_eq!(presence(), "unavailable");
        fs::create_dir_all(&index).unwrap();
        assert_eq!(presence(), "missing");
        symlink(&dst, &path).unwrap();
        assert_eq!(presence(), "changed");
        fs::create_dir_all(&dst).unwrap();
        assert_eq!(presence(), "present");
        assert_eq!(identity(), ("unavailable", None));
        fs::write(dst.join("vendor"), "0x8086\n").unwrap();
        fs::write(dst.join("device"), "0xabcd\n").unwrap();
        assert_eq!(identity(), ("match", Some([0x8086, 0xabcd])));
        fs::write(dst.join("device"), "0x0001\n").unwrap();
        assert_eq!(identity(), ("mismatch", Some([0x8086, 1])));
        fs::write(dst.join("device"), "0x000z\n").unwrap();
        assert_eq!(identity(), ("invalid", None));
        fs::remove_file(&path).unwrap();
        symlink(&index, &path).unwrap();
        assert_eq!(presence(), "invalid");
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

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

pub(crate) fn run(required: Option<&str>, profile_path: Option<&Path>) -> Result<(), String> {
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
    println!(
        r#"{{"schema_version":1,"process_architecture":{},{values},"resources":{}{profile_context}{profile}}}"#,
        json_string(std::env::consts::ARCH),
        resources()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

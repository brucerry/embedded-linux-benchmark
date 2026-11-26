//! Read-only PCI class inventory, not a GPU runtime or vendor health probe.
use crate::{inventory, pcie, sysfs::open_attribute};
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const MAX_PCI_ENTRIES: usize = 4096;

fn error(e: io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    }
}

fn identity(path: &Path) -> Result<(u64, u64), &'static str> {
    let m = fs::symlink_metadata(path).map_err(error)?;
    if !m.is_dir() {
        return Err("invalid");
    }
    Ok((m.dev(), m.ino()))
}

fn class(path: &Path) -> Result<u32, &'static str> {
    let mut s = String::new();
    open_attribute(path)
        .map_err(error)?
        .take(17)
        .read_to_string(&mut s)
        .map_err(error)?;
    let raw = s.trim().strip_prefix("0x").ok_or("invalid")?;
    if s.len() > 16 || raw.len() != 6 || !raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid");
    }
    u32::from_str_radix(raw, 16).map_err(|_| "invalid")
}

fn listing(index: &Path) -> Result<(Vec<String>, bool, usize), &'static str> {
    let mut names = Vec::new();
    let mut invalid = 0;
    let mut entries = fs::read_dir(index).map_err(error)?;
    for entry in entries.by_ref().take(MAX_PCI_ENTRIES) {
        let name = entry.map_err(error)?.file_name();
        match name
            .to_str()
            .and_then(|s| pcie::bdf(s).ok().filter(|id| id == s))
        {
            Some(name) => names.push(name),
            None => invalid += 1,
        }
    }
    names.sort_unstable();
    Ok((names, entries.next().is_some(), invalid))
}

fn id_json(v: Result<u16, &'static str>) -> String {
    match v {
        Ok(id) => format!(r#"{{"status":"observed","value":"{id:04x}"}}"#),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

fn scan(index: &Path, devices: &Path) -> Result<(bool, usize, Vec<String>), &'static str> {
    let initial = identity(index)?;
    let (names, capped, invalid_names) = listing(index)?;
    let mut unresolved = invalid_names;
    let mut rows = Vec::new();
    for name in &names {
        let row = (|| -> Result<Option<String>, &'static str> {
            let before = inventory::pci_entry(index, devices, name)?.ok_or("changed")?;
            let value = class(&before.0.join("class"))?;
            let kind = match value >> 16 {
                0x03 => "display_controller",
                0x12 => "processing_accelerator_class",
                _ => {
                    if inventory::pci_entry(index, devices, name)? != Some(before) {
                        return Err("changed");
                    }
                    return Ok(None);
                }
            };
            let vendor = inventory::pci_attr(&before.0.join("vendor"));
            let device = inventory::pci_attr(&before.0.join("device"));
            if inventory::pci_entry(index, devices, name)? != Some(before) {
                return Err("changed");
            }
            Ok(Some(format!(
                r#"{{"bdf":"{name}","class":"{value:06x}","kind":"{kind}","vendor":{},"device":{}}}"#,
                id_json(vendor),
                id_json(device)
            )))
        })();
        match row {
            Ok(Some(value)) => rows.push(value),
            Ok(None) => {}
            Err(_) => unresolved += 1,
        }
    }
    let again = listing(index)?;
    if identity(index)? != initial
        || again.0 != names
        || again.1 != capped
        || again.2 != invalid_names
    {
        return Err("changed");
    }
    Ok((capped, unresolved, rows))
}

fn render(index: &Path, devices: &Path) -> String {
    let (status, capped, unresolved, candidates) = match scan(index, devices) {
        Ok((capped, unresolved, rows)) => (
            if capped || unresolved > 0 {
                "partial"
            } else {
                "observed"
            },
            capped,
            unresolved.to_string(),
            rows.join(","),
        ),
        Err(status) => (status, false, "null".to_owned(), String::new()),
    };
    format!(
        r#"{{"schema_version":1,"kind":"pci_accelerator_inventory","status":"{status}","scan_capped":{capped},"unresolved_entries":{unresolved},"runtime_probe":"not_performed","candidates":[{candidates}]}}"#
    )
}

pub(crate) fn run() {
    println!(
        "{}",
        render(Path::new("/sys/bus/pci/devices"), Path::new("/sys/devices"))
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            loop {
                let n = NEXT.fetch_add(1, Ordering::Relaxed);
                let root =
                    std::env::temp_dir().join(format!("eld-accel-{}-{n}", std::process::id()));
                match fs::create_dir(&root) {
                    Ok(()) => return Self(root),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                    Err(e) => panic!("{e}"),
                }
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reports_class_candidates_but_does_not_follow_outside_devices() {
        let f = Fixture::new();
        let index = f.0.join("index");
        let devices = f.0.join("devices");
        fs::create_dir(&index).unwrap();
        fs::create_dir(&devices).unwrap();
        let add = |name: &str, place: &Path, value: &str| {
            let path = place.join(name);
            fs::create_dir(&path).unwrap();
            fs::write(path.join("class"), value).unwrap();
            fs::write(path.join("vendor"), "0x1234\n").unwrap();
            fs::write(path.join("device"), "0x5678\n").unwrap();
            symlink(&path, index.join(name)).unwrap();
        };
        add("0000:04:00.0", &devices, "0x120000\n");
        add("0000:02:00.0", &devices, "0x030200\n");
        add("0000:05:00.0", &devices, "0x020000\n");
        add("0000:06:00.0", &devices, "0x1200000\n");
        add("0000:07:00.0", &f.0, "0x120000\n");
        fs::remove_file(devices.join("0000:04:00.0/device")).unwrap();
        let json = render(&index, &devices);
        assert!(json.contains(r#""status":"partial","scan_capped":false,"unresolved_entries":2"#));
        assert!(json.contains(r#""class":"030200","kind":"display_controller""#));
        assert!(json.contains(r#""class":"120000","kind":"processing_accelerator_class""#));
        assert!(json.contains(r#""device":{"status":"unavailable"}"#));
        assert!(json.find("0000:02:00.0").unwrap() < json.find("0000:04:00.0").unwrap());
        for id in ["0000:05:00.0", "0000:06:00.0", "0000:07:00.0"] {
            assert!(!json.contains(id));
        }
    }

    #[test]
    fn missing_bus_is_not_an_observed_empty_bus() {
        let f = Fixture::new();
        let index = f.0.join("index");
        assert!(render(&index, &f.0).contains(r#""status":"unavailable""#));
        fs::create_dir(&index).unwrap();
        let empty = render(&index, &f.0);
        assert!(empty.contains(r#""status":"observed","scan_capped":false,"unresolved_entries":0"#));
        assert!(empty.ends_with(r#""candidates":[]}"#));
    }
}

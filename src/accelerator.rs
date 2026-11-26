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
    let (names, capped, mut unresolved) = listing(index)?;
    let mut rows = Vec::new();
    for name in &names {
        let row = (|| -> Result<Option<String>, &'static str> {
            let before = inventory::pci_entry(index, devices, name)?.ok_or("changed")?;
            let value = class(&before.0.join("class"))?;
            let kind = match value >> 16 {
                0x03 => "display_controller",
                0x12 => "processing_accelerator_class",
                _ => return Ok(None),
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
    if identity(index)? != initial || again.0 != names || again.1 != capped {
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

//! Registration evidence only: do not open character devices or vendor runtimes.
use crate::{inventory, pcie, sysfs};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_CLASS_ENTRIES: usize = 1024;

fn error(e: io::Error) -> &'static str {
    match e.kind() {
        io::ErrorKind::NotFound => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    }
}

fn directory(path: &Path) -> Result<(u64, u64), &'static str> {
    let m = fs::symlink_metadata(path).map_err(error)?;
    if !m.is_dir() {
        return Err("invalid");
    }
    Ok((m.dev(), m.ino()))
}

fn link(path: &Path, devices: &Path) -> Result<(PathBuf, u64, u64), &'static str> {
    if !fs::symlink_metadata(path).map_err(error)?.is_symlink() {
        return Err("invalid");
    }
    let target = fs::canonicalize(path).map_err(error)?;
    if target == devices || !target.starts_with(devices) || target.components().count() > 64 {
        return Err("invalid");
    }
    let (dev, ino) = directory(&target)?;
    Ok((target, dev, ino))
}

fn names(class_dir: &Path, prefix: &str) -> Result<(Vec<String>, bool), &'static str> {
    let mut rows = Vec::new();
    let mut listing = fs::read_dir(class_dir).map_err(error)?;
    for entry in listing.by_ref().take(MAX_CLASS_ENTRIES) {
        let name = entry.map_err(error)?.file_name();
        if let Some(s) = name.to_str() {
            if s.strip_prefix(prefix)
                .is_some_and(|suffix| suffix.len() <= 10 && sysfs::name_digits(suffix))
            {
                rows.push(s.to_owned());
            }
        }
    }
    rows.sort_unstable();
    Ok((rows, listing.next().is_some()))
}

fn registration(
    class_dir: &Path,
    devices: &Path,
    pci: &Path,
    name: &str,
) -> Result<String, &'static str> {
    let at = class_dir.join(name);
    let first = link(&at, devices)?;
    if first.0.file_name().and_then(|v| v.to_str()) != Some(name) {
        return Err("invalid");
    }
    let parent_at = first.0.join("device");
    let parent = link(&parent_at, devices);
    let relation = if let Ok(parent) = &parent {
        parent
            .0
            .file_name()
            .and_then(|v| v.to_str())
            .and_then(|v| pcie::bdf(v).ok().filter(|id| id == v))
            .filter(|id| {
                inventory::pci_entry(pci, devices, id)
                    .ok()
                    .flatten()
                    .as_ref()
                    == Some(parent)
            })
    } else {
        None
    };
    if link(&at, devices)? != first {
        return Err("changed");
    }
    let parent_status = if link(&parent_at, devices) != parent {
        "changed"
    } else {
        parent
            .as_ref()
            .map_or_else(|status| *status, |_| "observed")
    };
    let bdf = if parent_status == "observed" {
        relation.map_or_else(|| "null".to_owned(), |v| format!(r#""{v}""#))
    } else {
        "null".to_owned()
    };
    Ok(format!(
        r#"{{"name":"{name}","parent_status":"{parent_status}","pci_function":{bdf}}}"#
    ))
}

fn scan(
    class_dir: &Path,
    devices: &Path,
    pci: &Path,
    prefix: &str,
) -> Result<(bool, usize, Vec<String>), &'static str> {
    let initial = directory(class_dir)?;
    let (selected, capped) = names(class_dir, prefix)?;
    let mut items = Vec::new();
    let mut unresolved = 0;
    for name in &selected {
        match registration(class_dir, devices, pci, name) {
            Ok(item) => items.push(item),
            Err(_) => unresolved += 1,
        }
    }
    if directory(class_dir)? != initial || names(class_dir, prefix)? != (selected, capped) {
        return Err("changed");
    }
    Ok((capped, unresolved, items))
}

fn describe(class_dir: &Path, devices: &Path, pci: &Path, prefix: &str) -> String {
    let (status, capped, unresolved, rows) = match scan(class_dir, devices, pci, prefix) {
        Ok((cap, missing, rows)) => (
            if cap || missing > 0 {
                "partial"
            } else {
                "observed"
            },
            cap,
            missing.to_string(),
            rows.join(","),
        ),
        Err(status) => (status, false, "null".to_owned(), String::new()),
    };
    format!(
        r#"{{"status":"{status}","scan_capped":{capped},"unresolved_entries":{unresolved},"registrations":[{rows}]}}"#
    )
}

fn render(drm: &Path, accel: &Path, devices: &Path, pci: &Path) -> String {
    let graphics = describe(drm, devices, pci, "renderD");
    let compute = describe(accel, devices, pci, "accel");
    format!(
        r#"{{"schema_version":1,"kind":"linux_accelerator_interface_registration","runtime_probe":"not_performed","drm_render":{graphics},"compute_accel":{compute}}}"#
    )
}

pub(crate) fn run() {
    println!(
        "{}",
        render(
            Path::new("/sys/class/drm"),
            Path::new("/sys/class/accel"),
            Path::new("/sys/devices"),
            Path::new("/sys/bus/pci/devices")
        )
    );
}

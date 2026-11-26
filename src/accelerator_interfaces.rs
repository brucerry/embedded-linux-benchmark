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
            if s.strip_prefix(prefix).map_or(false, |suffix| {
                suffix.len() <= 10 && sysfs::name_digits(suffix)
            }) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn distinguishes_pci_platform_spoofed_and_missing_class() {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = loop {
            let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("eld-iface-{}-{suffix}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => break path,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => panic!("{e}"),
            }
        };
        struct Remove(PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _guard = Remove(root.clone());
        let class_drm = root.join("drm");
        let class_accel = root.join("accel");
        let pci = root.join("pci");
        let devices = root.join("devices");
        let gpu = devices.join("pci0000:00/0000:00:02.0");
        let platform = devices.join("platform/engine");
        let alias = devices.join("platform/0000:00:02.0");
        let rendered = gpu.join("drm/renderD128");
        let registered = platform.join("accel/accel0");
        let spoofed = alias.join("drm/renderD129");
        let outside = root.join("renderD130");
        for d in [&class_drm, &pci, &rendered, &registered, &spoofed, &outside] {
            fs::create_dir_all(d).unwrap();
        }
        symlink(&gpu, pci.join("0000:00:02.0")).unwrap();
        symlink(&gpu, rendered.join("device")).unwrap();
        symlink(&platform, registered.join("device")).unwrap();
        symlink(&alias, spoofed.join("device")).unwrap();
        symlink(&rendered, class_drm.join("renderD128")).unwrap();
        symlink(&spoofed, class_drm.join("renderD129")).unwrap();
        symlink(&outside, class_drm.join("renderD130")).unwrap();
        let missing = render(&class_drm, &class_accel, &devices, &pci);
        assert!(missing.contains(r#""compute_accel":{"status":"unavailable""#));
        fs::create_dir(&class_accel).unwrap();
        let empty = render(&class_drm, &class_accel, &devices, &pci);
        assert!(empty.contains(r#""compute_accel":{"status":"observed","scan_capped":false,"unresolved_entries":0,"registrations":[]"#));
        symlink(&registered, class_accel.join("accel0")).unwrap();
        let observed = render(&class_drm, &class_accel, &devices, &pci);
        assert!(observed.contains(
            r#""name":"renderD128","parent_status":"observed","pci_function":"0000:00:02.0""#
        ));
        assert!(observed
            .contains(r#""name":"renderD129","parent_status":"observed","pci_function":null"#));
        assert!(
            observed.contains(r#""name":"accel0","parent_status":"observed","pci_function":null"#)
        );
        assert!(!observed.contains("renderD130"));
        assert!(observed.contains(
            r#""drm_render":{"status":"partial","scan_capped":false,"unresolved_entries":1"#
        ));
    }
}

use crate::common::Deadline;
use crate::pcie;
use crate::sysfs::{name_digits, open_attribute};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const SCAN_LIMIT: usize = 256;
const DEVICE_LIMIT: usize = 32;
const VALUE_LIMIT: u64 = 64;

#[derive(Clone, Copy)]
enum Kind {
    Pcie,
    Usb,
    Hdmi,
}

impl Kind {
    fn valid(self, name: &str) -> bool {
        match self {
            Self::Pcie => pcie::bdf(name).is_ok(),
            Self::Usb => match name.split_once('-') {
                // Exclude controller roots and interface names such as 1-2:1.0.
                Some((bus, path)) => name_digits(bus) && path.split('.').all(name_digits),
                None => false,
            },
            Self::Hdmi => name
                .strip_prefix("card")
                .and_then(|name| name.split_once("-HDMI-"))
                .and_then(|(card, rest)| rest.split_once('-').map(|(typ, n)| (card, typ, n)))
                .map_or(false, |(card, typ, n)| {
                    name_digits(card) && matches!(typ, "A" | "B") && name_digits(n)
                }),
        }
    }

    fn attribute(self) -> &'static str {
        match self {
            Self::Pcie => "current_link_width",
            Self::Usb => "speed",
            // An enabled-state snapshot is passive. Do not detect or reprobe a display.
            Self::Hdmi => "enabled",
        }
    }

    fn valid_value(self, value: &str) -> bool {
        match self {
            Self::Pcie => value.parse::<u32>().map_or(false, |width| width > 0),
            Self::Usb => value
                .parse::<f64>()
                .map_or(false, |speed| speed.is_finite() && speed > 0.0),
            Self::Hdmi => matches!(value, "enabled" | "disabled"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pcie => "PCIe links",
            Self::Usb => "USB link speeds",
            Self::Hdmi => "HDMI encoder states",
        }
    }
}

struct Group {
    kind: Kind,
    paths: Vec<PathBuf>,
    unavailable: bool,
    limited: bool,
    observed: u64,
    skipped: u64,
}

impl Group {
    fn discover(kind: Kind, directory: &Path, deadline: Deadline) -> Self {
        let mut group = Self {
            kind,
            paths: Vec::new(),
            unavailable: false,
            limited: false,
            observed: 0,
            skipped: 0,
        };
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(_) => {
                group.unavailable = true;
                return group;
            }
        };
        for (index, entry) in entries.enumerate().take(SCAN_LIMIT + 1) {
            if index == SCAN_LIMIT || deadline.check().is_err() {
                group.limited = true;
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    group.unavailable = true;
                    continue;
                }
            };
            if entry
                .file_name()
                .to_str()
                .map_or(false, |name| kind.valid(name))
            {
                if group.paths.len() == DEVICE_LIMIT {
                    group.limited = true;
                    break;
                }
                group.paths.push(entry.path().join(kind.attribute()));
            }
        }
        group.paths.sort_unstable();
        group
    }

    fn observe(&mut self, deadline: Deadline) -> bool {
        for path in &self.paths {
            if deadline.check().is_err() {
                return false;
            }
            if read_value(self.kind, path) {
                self.observed = self.observed.saturating_add(1);
            } else {
                self.skipped = self.skipped.saturating_add(1);
            }
        }
        deadline.check().is_ok()
    }
}

fn read_value(kind: Kind, path: &Path) -> bool {
    let mut value = String::new();
    open_attribute(path)
        .and_then(|file| file.take(VALUE_LIMIT + 1).read_to_string(&mut value))
        .map_or(false, |size| {
            size <= VALUE_LIMIT as usize && kind.valid_value(value.trim())
        })
}

pub(super) struct Probes([Group; 3]);

impl Probes {
    pub(super) fn discover(deadline: Deadline) -> Self {
        Self([
            Group::discover(Kind::Pcie, Path::new("/sys/bus/pci/devices"), deadline),
            Group::discover(Kind::Usb, Path::new("/sys/bus/usb/devices"), deadline),
            Group::discover(Kind::Hdmi, Path::new("/sys/class/drm"), deadline),
        ])
    }

    pub(super) fn observe(&mut self, deadline: Deadline) -> bool {
        self.0.iter_mut().all(|group| group.observe(deadline))
    }

    pub(super) fn describe(&self) -> String {
        self.0
            .iter()
            .map(|group| {
                let base = if group.paths.is_empty() {
                    "unavailable".to_owned()
                } else {
                    format!("{} observed, {} skipped", group.observed, group.skipped)
                };
                format!(
                    "{}: {base}{}{}",
                    group.kind.label(),
                    if group.limited { " (scan capped)" } else { "" },
                    if group.unavailable && !group.paths.is_empty() {
                        " (directory incomplete)"
                    } else {
                        ""
                    },
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_known_device_names_and_passive_values() {
        assert!(Kind::Pcie.valid("0000:01:1f.0"));
        assert!(!Kind::Pcie.valid("rescan"));
        assert!(Kind::Usb.valid("1-2.3.4"));
        assert!(!Kind::Usb.valid("1-2:1.0"));
        assert!(!Kind::Usb.valid("usb1"));
        assert!(Kind::Hdmi.valid("card0-HDMI-A-1"));
        assert!(!Kind::Hdmi.valid("card0-DP-1"));
        assert!(Kind::Usb.valid_value("480"));
        assert!(!Kind::Usb.valid_value("NaN"));
        assert!(Kind::Hdmi.valid_value("disabled"));
        assert!(!Kind::Pcie.valid_value("Unknown"));
    }

    #[test]
    fn bounded_scan_and_read_only_attributes_distinguish_missing_and_valid() {
        let root =
            std::env::temp_dir().join(format!("benchmark-stress-probes-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        for i in 1..=DEVICE_LIMIT + 1 {
            let device = root.join(i.to_string() + "-1");
            fs::create_dir(&device).unwrap();
            fs::write(device.join("speed"), "480\n").unwrap();
            fs::write(device.join("authorized"), "1\n").unwrap();
        }
        let deadline = Deadline::after(std::time::Duration::from_secs(2));
        let mut scan = Group::discover(Kind::Usb, &root, deadline);
        assert!(scan.limited);
        assert_eq!(scan.paths.len(), DEVICE_LIMIT);
        let invalid = scan.paths[0].clone();
        fs::write(&invalid, "1".repeat(VALUE_LIMIT as usize + 1)).unwrap();
        let missing = scan.paths[1].clone();
        fs::remove_file(missing).unwrap();
        assert!(scan.observe(deadline));
        assert_eq!(scan.skipped, 2);
        assert_eq!(scan.observed, DEVICE_LIMIT as u64 - 2);
        assert_eq!(
            fs::read_to_string(root.join("1-1/authorized")).unwrap(),
            "1\n"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

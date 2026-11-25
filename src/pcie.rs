use crate::sysfs::open_attribute;
use std::io::{self, Read};
use std::path::Path;

const LINK_ATTRIBUTES: [(&str, &str); 4] = [
    ("current_link_speed", "Negotiated link speed"),
    ("current_link_width", "Negotiated link width"),
    ("max_link_speed", "Maximum link speed"),
    ("max_link_width", "Maximum link width"),
];

pub(crate) fn bdf(value: &str) -> Result<String, String> {
    let valid = || {
        let (domain, rest) = value.split_once(':')?;
        let (bus, rest) = rest.split_once(':')?;
        let (device, function) = rest.split_once('.')?;
        let hex = |part: &str, width: usize| {
            if part.len() == width && part.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                u16::from_str_radix(part, 16).ok()
            } else {
                None
            }
        };
        hex(domain, 4)?;
        hex(bus, 2)?;
        (hex(device, 2)? <= 31 && hex(function, 1)? <= 7).then_some(())
    };
    valid().ok_or("expected a complete PCI device ID like '0000:01:00.0'")?;
    Ok(value.to_ascii_lowercase())
}

fn read_link(device: &Path) -> Result<[Option<String>; 4], String> {
    if !device.is_dir() {
        return Err(format!(
            "PCI device {} is unavailable in sysfs",
            device.display()
        ));
    }
    let mut values = [None, None, None, None];
    for ((attribute, _), value) in LINK_ATTRIBUTES.into_iter().zip(&mut values) {
        let path = device.join(attribute);
        let file = match open_attribute(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot open PCI {attribute}: {error}")),
        };
        let mut content = String::new();
        file.take(129)
            .read_to_string(&mut content)
            .map_err(|error| format!("cannot read PCI {attribute}: {error}"))?;
        let text = content.trim();
        if content.len() > 128
            || text.is_empty()
            || !text
                .bytes()
                .all(|byte| byte.is_ascii_graphic() || byte == b' ')
        {
            return Err(format!("invalid PCI {attribute} value"));
        }
        *value = Some(text.to_owned());
    }
    if values.iter().all(Option::is_none) {
        return Err("PCIe link attributes are unavailable for this device".into());
    }
    Ok(values)
}

pub(crate) fn link(device: &str) -> Result<(), String> {
    let device = bdf(device)?;
    let link = read_link(&Path::new("/sys/bus/pci/devices").join(&device))?;
    report_line!("PCIe link information (read-only sysfs)");
    report_line!("Device: {device}");
    for ((_, label), value) in LINK_ATTRIBUTES.into_iter().zip(&link) {
        report_line!("{label}: {}", value.as_deref().unwrap_or("unavailable"));
    }
    Ok(())
}

pub(crate) fn throughput(device: &str, fixture: bool) -> Result<(), String> {
    bdf(device)?;
    if !fixture {
        return Err(
            "PCIe throughput requires '--fixture' to acknowledge a dedicated endpoint test device"
                .into(),
        );
    }
    // Reviewed upstream pci_endpoint_test transfer ioctls wait uninterruptibly for completion.
    // The stock UAPI cannot negotiate a bounded transfer.
    Err("PCIe endpoint throughput unavailable: the stock pci_endpoint_test transfer ABI has no bounded completion timeout".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn link_reads_only_known_attributes_and_preserves_unavailable_fields() {
        let root = std::env::temp_dir().join(format!("benchmark-pci-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let missing = read_link(&root).unwrap_err();
        fs::write(root.join("current_link_speed"), "8.0 GT/s PCIe\n").unwrap();
        fs::write(root.join("current_link_width"), "4\n").unwrap();
        fs::write(root.join("max_link_width"), "8\n").unwrap();
        let observed = read_link(&root).unwrap();
        fs::write(root.join("max_link_speed"), b"8.0 GT/s\n\x1b[0m").unwrap();
        let malformed = read_link(&root).unwrap_err();
        fs::remove_dir_all(&root).unwrap();
        assert!(missing.contains("unavailable"));
        assert_eq!(
            observed.map(|text| text.unwrap_or_default()),
            ["8.0 GT/s PCIe", "4", "", "8"]
        );
        assert!(malformed.contains("invalid PCI max_link_speed"));
        assert_eq!(bdf("00AB:01:1F.7"), Ok("00ab:01:1f.7".into()));
        for malformed in ["01:00.0", "0000:01:20.0", "0000:01:00.8", "../0000:01:00.0"] {
            assert!(bdf(malformed).is_err());
        }
        assert!(throughput("0000:01:00.0", true)
            .unwrap_err()
            .contains("no bounded completion"));
    }
}

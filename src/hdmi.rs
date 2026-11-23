use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

fn connector_name(name: &str) -> bool {
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    let (card, rest) = match name
        .strip_prefix("card")
        .and_then(|name| name.split_once("-HDMI-"))
    {
        Some(parts) => parts,
        None => return false,
    };
    let (kind, number) = match rest.split_once('-') {
        Some(parts) => parts,
        None => return false,
    };
    digits(card) && matches!(kind, "A" | "B") && digits(number)
}

fn read_attribute(connector: &Path, name: &str) -> Result<String, String> {
    let path = connector.join(name);
    let error = |error: std::io::Error| format!("cannot read DRM {name}: {error}");
    if !fs::symlink_metadata(&path).map_err(error)?.is_file() {
        return Err(format!("DRM {name} is not a regular sysfs attribute"));
    }
    let mut value = String::new();
    File::open(path)
        .map_err(error)?
        .take(16_385)
        .read_to_string(&mut value)
        .map_err(error)?;
    if value.len() > 16_384
        || !value
            .bytes()
            .all(|byte| byte == b'\n' || byte == b' ' || byte.is_ascii_graphic())
    {
        return Err(format!("invalid DRM {name} data"));
    }
    Ok(value)
}

fn observe(root: &Path, connector: &str) -> Result<(String, String, String), String> {
    if !connector_name(connector) {
        return Err("HDMI requires a connector name such as 'card0-HDMI-A-1'".into());
    }
    let path = root.join(connector);
    let status = read_attribute(&path, "status")?;
    let enabled = read_attribute(&path, "enabled")?;
    let modes = read_attribute(&path, "modes")?;
    Ok((status, enabled, modes))
}

pub(crate) fn status(connector: &str) -> Result<(), String> {
    let (status, enabled, modes) = observe(Path::new("/sys/class/drm"), connector)?;
    println!("HDMI connector (read-only DRM sysfs)");
    println!("Connector: {connector}");
    println!("Connection status: {}", status.trim());
    println!("Encoder status: {}", enabled.trim());
    if modes.trim().is_empty() {
        println!("Advertised modes: none");
    } else {
        println!("Advertised modes:");
        for mode in modes.lines() {
            println!("  {mode}");
        }
    }
    println!("Active timing is unavailable from these connector attributes.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_observes_only_named_hdmi_connector_attributes() {
        let root = std::env::temp_dir().join(format!("benchmark-drm-{}", std::process::id()));
        let connector = "card0-HDMI-A-1";
        let path = root.join(connector);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join("status"), "connected\n").unwrap();
        fs::write(path.join("enabled"), "enabled\n").unwrap();
        fs::write(path.join("modes"), "1920x1080\n1280x720\n").unwrap();
        let result = observe(&root, connector);
        let invalid = observe(&root, "../card0-HDMI-A-1");
        fs::write(path.join("modes"), "\x1b[31m\n").unwrap();
        let malformed = observe(&root, connector);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            result.unwrap(),
            (
                "connected\n".into(),
                "enabled\n".into(),
                "1920x1080\n1280x720\n".into()
            )
        );
        assert!(invalid.is_err());
        assert!(malformed.is_err());
    }
}

use crate::{
    pcie_health::json_string,
    sysfs::{attribute_status, bounded_text},
};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

fn card_host(name: &str) -> Option<&str> {
    let (host, addr) = name.split_once(':')?;
    let index = host.strip_prefix("mmc")?;
    if index.is_empty()
        || index.len() > 7
        || !index.bytes().all(|b| b.is_ascii_digit())
        || (index.len() != 1 && index.starts_with('0'))
        || addr.len() != 4
        || !addr
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return None;
    }
    Some(host)
}

fn byte(value: &str) -> Result<u8, &'static str> {
    let text = value.strip_prefix("0x").ok_or("invalid")?;
    if text.len() != 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid");
    }
    u8::from_str_radix(text, 16).map_err(|_| "invalid")
}

fn lifetime(value: &str) -> Result<String, &'static str> {
    let mut tokens = value.split_ascii_whitespace();
    let a = byte(tokens.next().ok_or("invalid")?)?;
    let b = byte(tokens.next().ok_or("invalid")?)?;
    if tokens.next().is_some() {
        return Err("invalid");
    }
    Ok(format!(r#"{{"type_a_raw":{a},"type_b_raw":{b}}}"#))
}

fn field(value: Result<String, &'static str>) -> String {
    match value {
        Ok(value) => format!(r#"{{"status":"observed","value":{value}}}"#),
        Err(status) => format!(r#"{{"status":"{status}"}}"#),
    }
}

fn report(index: &Path, devices: &Path, name: &str) -> Result<String, String> {
    let host = card_host(name).ok_or("expected an MMC card id such as mmc0:0001")?;
    let entry = index.join(name);
    let identity = || {
        let target = fs::canonicalize(&entry).map_err(attribute_status)?;
        if !target.starts_with(devices)
            || target.file_name().and_then(|p| p.to_str()) != Some(name)
            || target
                .parent()
                .and_then(Path::file_name)
                .and_then(|p| p.to_str())
                != Some(host)
        {
            return Err("invalid");
        }
        let meta = fs::symlink_metadata(&target).map_err(attribute_status)?;
        if !meta.is_dir() {
            return Err("invalid");
        }
        Ok((target, meta.dev(), meta.ino()))
    };
    let card = json_string(name);
    let withheld = |status| format!(r#"{{"schema_version":1,"card":{card},"status":"{status}"}}"#);
    let initial = match identity() {
        Ok(initial) => initial,
        Err(reason) => return Ok(withheld(reason)),
    };
    let card_type = bounded_text(&initial.0.join("type"));
    let result = match card_type.as_deref() {
        Ok("MMC") => {
            let eol = field(
                bounded_text(&initial.0.join("pre_eol_info"))
                    .and_then(|text| byte(&text))
                    .map(|n| n.to_string()),
            );
            let life =
                field(bounded_text(&initial.0.join("life_time")).and_then(|text| lifetime(&text)));
            format!(
                r#"{{"schema_version":1,"card":{card},"status":"observed","card_type":"MMC","pre_eol_info_raw":{eol},"life_time_raw":{life}}}"#
            )
        }
        Ok("SD" | "SDIO" | "SDcombo") => withheld("unsupported_card_type"),
        Ok(_) => withheld("invalid"),
        Err(reason) => withheld(reason),
    };
    if identity().ok().as_ref() != Some(&initial) {
        return Ok(withheld("changed"));
    }
    Ok(result)
}

pub(crate) fn run(name: &str) -> Result<(), String> {
    println!(
        "{}",
        report(
            Path::new("/sys/bus/mmc/devices"),
            Path::new("/sys/devices"),
            name
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn only_reads_named_whitelisted_mmc_health_fields() {
        let root = std::env::temp_dir().join(format!("eld-mmc-health-{}", std::process::id()));
        let index = root.join("index");
        let devices = root.join("devices");
        let card = devices.join("mmc_host/mmc0/mmc0:000a");
        fs::create_dir_all(&card).unwrap();
        fs::create_dir(&index).unwrap();
        symlink(&card, index.join("mmc0:000a")).unwrap();
        fs::write(card.join("type"), "MMC\n").unwrap();
        fs::write(card.join("life_time"), "0x00 0x0a\n").unwrap();
        fs::write(card.join("pre_eol_info"), "0x02\n").unwrap();
        fs::write(card.join("cid"), "must-not-leak\n").unwrap();
        let value = report(&index, &devices, "mmc0:000a").unwrap();
        assert!(value.contains(r#""pre_eol_info_raw":{"status":"observed","value":2}"#));
        assert!(value.contains(
            r#""life_time_raw":{"status":"observed","value":{"type_a_raw":0,"type_b_raw":10}}"#
        ));
        assert!(!value.contains("must-not-leak"));
        fs::write(card.join("life_time"), "0x00 0xa\n").unwrap();
        fs::remove_file(card.join("pre_eol_info")).unwrap();
        let value = report(&index, &devices, "mmc0:000a").unwrap();
        assert!(value.contains(r#""pre_eol_info_raw":{"status":"unavailable"}"#));
        assert!(value.contains(r#""life_time_raw":{"status":"invalid"}"#));
        fs::write(card.join("type"), "SD\n").unwrap();
        assert!(report(&index, &devices, "mmc0:000a")
            .unwrap()
            .contains("unsupported_card_type"));
        symlink(&card, index.join("mmc1:000a")).unwrap();
        assert!(report(&index, &devices, "mmc1:000a")
            .unwrap()
            .contains(r#""status":"invalid""#));
        for name in ["../mmc0:000a", "mmcblk0", "mmc01:000a", "mmc0:000A"] {
            assert!(report(&index, &devices, name).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }
}

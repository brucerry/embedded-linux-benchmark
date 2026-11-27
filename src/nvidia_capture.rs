//! Offline normalized NVML record only. Does not link to NVML or open a device.
pub(crate) const MAX_CAPTURE_BYTES: usize = 160;

fn bdf(input: &str) -> Result<String, String> {
    // NVML commonly prints eight domain digits. Accept only lossless
    // normalization to the Linux four-digit domain form.
    let normalized = if input.as_bytes().get(8) == Some(&b':') && input.starts_with("0000") {
        &input[4..]
    } else {
        input
    };
    crate::pcie::bdf(normalized)
}

fn metric(input: &str, limit: u64) -> Result<String, String> {
    if matches!(input, "unsupported" | "unavailable" | "inaccessible") {
        return Ok(format!(r#"{{"status":"{input}"}}"#));
    }
    if !crate::sysfs::name_digits(input) || input.len() > 20 {
        return Err(
            "expected a bounded unsigned NVML value or an explicit unavailable state".into(),
        );
    }
    let value = input
        .parse::<u64>()
        .ok()
        .filter(|n| *n <= limit)
        .ok_or("NVML metric exceeds its integer width")?;
    Ok(format!(r#"{{"status":"observed","value":"{value}"}}"#))
}

pub(crate) fn decode(expected_bdf: &str, capture: &str) -> Result<String, String> {
    if capture.len() > MAX_CAPTURE_BYTES
        || !capture.is_ascii()
        || capture.bytes().any(|b| b.is_ascii_control())
    {
        return Err("expected one bounded ASCII NVIDIA capture record".into());
    }
    let fields: Vec<_> = capture.split(',').collect();
    let [version, bus, temp, power, corrected, uncorrected] = fields.as_slice() else {
        return Err("expected NVML normalized version, BDF and four metrics".into());
    };
    if *version != "eld_nvml_v1" {
        return Err("unrecognized normalized NVML record version".into());
    }
    let bus = bdf(bus)?;
    if bus != bdf(expected_bdf)? {
        return Err("capture PCI device differs from explicitly selected device".into());
    }
    let temp = metric(temp, u32::MAX.into())?;
    let power = metric(power, u32::MAX.into())?;
    let corrected = metric(corrected, u64::MAX)?;
    let uncorrected = metric(uncorrected, u64::MAX)?;
    Ok(format!(
        r#"{{"schema_version":1,"kind":"nvidia_nvml_capture","source":"provided_normalized_text","capture_identity":"unverified","live_query":"not_performed","bdf":"{bus}","temperature_celsius":{temp},"power_milliwatts":{power},"ecc_scope":"volatile_total","corrected_ecc":{corrected},"uncorrected_ecc":{uncorrected}}}"#,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unknown_zero_and_exact_wide_ecc() {
        let json = decode(
            "0000:65:00.0",
            "eld_nvml_v1,00000000:65:00.0,0,unsupported,9007199254740993,inaccessible",
        )
        .unwrap();
        assert!(json.contains(r#""bdf":"0000:65:00.0""#));
        assert!(json.contains(r#""temperature_celsius":{"status":"observed","value":"0"}"#));
        assert!(json.contains(r#""power_milliwatts":{"status":"unsupported"}"#));
        assert!(
            json.contains(r#""corrected_ecc":{"status":"observed","value":"9007199254740993"}"#)
        );
        assert!(json.contains(r#""uncorrected_ecc":{"status":"inaccessible"}"#));
        assert!(json.contains(r#""live_query":"not_performed""#));
    }

    #[test]
    fn refuses_mismatched_lossy_or_malformed_records() {
        for capture in [
            "eld_nvml_v1,0000:66:00.0,40,30,0,0",
            "eld_nvml_v1,00010000:65:00.0,40,30,0,0",
            "eld_nvml_v1,0000:65:00.0,40,4294967296,0,0",
            "eld_nvml_v1,0000:65:00.0,40,30,18446744073709551616,0",
            "eld_nvml_v1,0000:65:00.0,40,30,0,0\n",
            "eld_nvml_v2,0000:65:00.0,40,30,0,0",
        ] {
            assert!(decode("0000:65:00.0", capture).is_err(), "{capture:?}");
        }
        assert!(decode("0000:65:00.0", &"0".repeat(MAX_CAPTURE_BYTES + 1)).is_err());
    }
}

//! Decode literal, previously captured text. Never access a GPU or a file.
pub(crate) const LIMIT: usize = 128;

pub(crate) fn decode(text: &str) -> Result<String, String> {
    if text.len() > LIMIT || !text.is_ascii() {
        return Err("expected at most 128 bytes of ASCII capture text".into());
    }
    let text = text.trim();
    const BASE: &str = r#"{"schema_version":1,"kind":"amdgpu_ras_capture","source":"provided_text","capture_identity":"unverified","live_query":"not_performed""#;
    if text == "Query currently inaccessible" {
        return Ok(format!(r#"{BASE},"status":"unavailable"}}"#));
    }
    let (mut ce, mut ue) = (None, None);
    for line in text.lines() {
        let (label, value) = line.split_once(':').ok_or("invalid RAS capture")?;
        let number = value.trim();
        if !crate::sysfs::name_digits(number) {
            return Err("invalid RAS capture".into());
        }
        let target = match label {
            "ce" => &mut ce,
            "ue" => &mut ue,
            _ => return Err("invalid RAS capture".into()),
        };
        if target
            .replace(number.parse::<u64>().map_err(|_| "invalid RAS capture")?)
            .is_some()
        {
            return Err("duplicate RAS counter".into());
        }
    }
    let (ce, ue) = (
        ce.ok_or("missing ce counter")?,
        ue.ok_or("missing ue counter")?,
    );
    Ok(format!(
        r#"{BASE},"status":"decoded","corrected_errors":"{ce}","uncorrected_errors":"{ue}"}}"#
    ))
}

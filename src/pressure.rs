use crate::sysfs::{attribute_status, open_attribute};
use std::{io::Read, path::Path};

fn ratio(field: &str) -> Option<&str> {
    let (whole, frac) = field.split_once('.')?;
    if whole.is_empty()
        || whole.len() > 3
        || frac.len() != 2
        || !whole
            .bytes()
            .chain(frac.bytes())
            .all(|c| c.is_ascii_digit())
        || (whole.len() > 1 && whole.starts_with('0'))
    {
        return None;
    }
    let hundredths = whole.parse::<u16>().ok()? * 100 + frac.parse::<u16>().ok()?;
    (hundredths <= 10_000).then_some(field)
}

fn row(fields: &mut std::str::SplitAsciiWhitespace<'_>) -> Option<String> {
    let a = ratio(fields.next()?.strip_prefix("avg10=")?)?;
    let b = ratio(fields.next()?.strip_prefix("avg60=")?)?;
    let c = ratio(fields.next()?.strip_prefix("avg300=")?)?;
    let raw = fields.next()?.strip_prefix("total=")?;
    if fields.next().is_some() || raw.is_empty() || !raw.bytes().all(|x| x.is_ascii_digit()) {
        return None;
    }
    let total = raw.parse::<u64>().ok()?;
    Some(format!(
        r#"{{"status":"observed","avg10_percent":"{a}","avg60_percent":"{b}","avg300_percent":"{c}","total_stall_us":"{total}"}}"#
    ))
}

fn parse(text: &str, cpu: bool) -> Option<String> {
    let mut some = None;
    let mut full = None;
    for line in text.lines() {
        let mut fields = line.split_ascii_whitespace();
        match fields.next()? {
            "some" if some.is_none() => some = Some(row(&mut fields)?),
            "full" if full.is_none() => full = Some(row(&mut fields)?),
            _ => return None,
        }
    }
    let some = some?;
    // System-level CPU "full" is undefined, even when the kernel prints zeros.
    let unavailable = r#"{"status":"unavailable"}"#;
    let full = if cpu {
        r#"{"status":"not_applicable"}"#
    } else {
        full.as_deref().unwrap_or(unavailable)
    };
    Some(format!(
        r#"{{"status":"observed","some":{some},"full":{full}}}"#
    ))
}

fn read(dir: &Path, resource: &str) -> String {
    let result = (|| {
        let mut text = String::new();
        open_attribute(&dir.join(resource))
            .map_err(attribute_status)?
            .take(1025)
            .read_to_string(&mut text)
            .map_err(attribute_status)?;
        if text.len() > 1024 {
            return Err("invalid");
        }
        parse(&text, resource == "cpu").ok_or("invalid")
    })();
    result.unwrap_or_else(|status| format!(r#"{{"status":"{status}"}}"#))
}

pub(crate) fn snapshot() -> String {
    let dir = Path::new("/proc/pressure");
    format!(
        r#"{{"schema_version":1,"scope":"system_proc_pressure","cpu":{},"memory":{},"io":{}}}"#,
        read(dir, "cpu"),
        read(dir, "memory"),
        read(dir, "io")
    )
}

pub(crate) fn run() {
    println!("{}", snapshot());
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOME: &str = "some avg10=0.00 avg60=1.20 avg300=100.00 total=18446744073709551615\n";
    const FULL: &str = "full avg10=0.00 avg60=0.00 avg300=0.00 total=0\n";
    #[test]
    fn pressure_preserves_precision_and_system_cpu_full_is_not_a_measurement() {
        let both = format!("{SOME}{FULL}");
        let cpu = parse(&both, true).unwrap();
        let memory = parse(&both, false).unwrap();
        assert!(cpu.contains(r#""full":{"status":"not_applicable"}"#));
        assert!(cpu.contains(r#""total_stall_us":"18446744073709551615""#));
        assert!(cpu.contains(r#""avg60_percent":"1.20""#));
        assert!(memory.contains(r#""full":{"status":"observed""#));
        assert!(memory.ends_with(r#""total_stall_us":"0"}}"#));
        assert!(parse(SOME, false)
            .unwrap()
            .contains(r#""full":{"status":"unavailable"}"#));
        assert!(parse(SOME, true).unwrap().contains("not_applicable"));
    }
    #[test]
    fn pressure_rejects_missing_duplicate_or_invalid_fields() {
        for text in [
            String::new(),
            FULL.to_owned(),
            format!("{SOME}{SOME}"),
            SOME.replace("100.00", "100.01"),
            SOME.replace("0.00", "NaN"),
            SOME.replace("18446744073709551615", "18446744073709551616"),
        ] {
            assert!(parse(&text, false).is_none(), "{text:?}");
        }
        assert_eq!(
            read(Path::new("/not-a-real-engine-fixture-pressure"), "cpu"),
            r#"{"status":"unavailable"}"#
        );
    }
}

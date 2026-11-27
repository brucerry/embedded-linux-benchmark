use crate::sysfs::open_attribute;
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::Path,
    time::{Duration, Instant},
};

const ROOT: &str = "/sys/devices/system/edac/mc";
pub(crate) const MAX_WAIT_MS: u64 = 2000;
const COUNTERS: [&str; 2] = ["ce_count", "ue_count"];
type Value = Result<u64, &'static str>;

struct Snapshot {
    identity: (u64, u64),
    reset_age: Value,
    errors: [Value; COUNTERS.len()],
}

fn controller(name: &str) -> bool {
    let Some(n) = name.strip_prefix("mc") else {
        return false;
    };
    valid_suffix(n)
}

fn valid_suffix(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 7
        && n.bytes().all(|b| b.is_ascii_digit())
        && (n.len() == 1 || !n.starts_with('0'))
}

fn status(error: io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::Unsupported => "unavailable",
        io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "invalid",
    }
}

fn counter(p: &Path) -> Value {
    let mut text = String::new();
    open_attribute(p)
        .map_err(status)?
        .take(33)
        .read_to_string(&mut text)
        .map_err(status)?;
    let value = text.strip_suffix('\n').unwrap_or(&text);
    if text.len() > 32 || value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err("invalid");
    }
    value.parse::<u64>().map_err(|_| "invalid")
}

fn identity(dir: &Path) -> Result<(u64, u64), &'static str> {
    let info = fs::symlink_metadata(dir).map_err(status)?;
    if !info.is_dir() {
        return Err("invalid");
    }
    Ok((info.dev(), info.ino()))
}

fn snapshot(dir: &Path) -> Result<Snapshot, &'static str> {
    let initial = identity(dir)?;
    let last_reset = dir.join("seconds_since_reset");
    let age_before = counter(&last_reset);
    let errors = COUNTERS.map(|name| counter(&dir.join(name)));
    let age_after = counter(&last_reset);
    if identity(dir).ok() != Some(initial) {
        return Err("controller_changed");
    }
    let reset_age = match (age_before, age_after) {
        (Ok(a), Ok(b)) if b >= a => Ok(b),
        (Ok(_), Ok(_)) => return Err("reset_age_decreased"),
        (Err(a), Err(b)) if a == b => Err(a),
        _ => Err("observation_state_changed"),
    };
    Ok(Snapshot {
        identity: initial,
        reset_age,
        errors,
    })
}

fn value_json(value: Value) -> String {
    match value {
        Ok(n) => format!(r#"{{"status":"observed","value":"{n}"}}"#),
        Err(s) => format!(r#"{{"status":"{s}"}}"#),
    }
}

fn render_error_values(errors: &[Value; 2]) -> String {
    format!(
        r#"{{"ce_count":{},"ue_count":{}}}"#,
        value_json(errors[0]),
        value_json(errors[1])
    )
}

fn render(snapshot: &Result<Snapshot, &'static str>) -> String {
    match snapshot {
        Ok(s) => format!(
            r#"{{"status":"observed","seconds_since_reset":{},"counters":{}}}"#,
            value_json(s.reset_age),
            render_error_values(&s.errors)
        ),
        Err(s) => format!(r#"{{"status":"{s}"}}"#),
    }
}

fn deltas(a: &Snapshot, b: &Snapshot, whole_window: Duration) -> Result<[Value; 2], &'static str> {
    if a.identity != b.identity {
        return Err("controller_changed");
    }
    let (Ok(age_a), Ok(age_b)) = (a.reset_age, b.reset_age) else {
        return Err("reset_age_unavailable");
    };
    if age_b < age_a {
        return Err("reset_age_decreased");
    }
    // The kernel exposes integer seconds. Refuse if the last reset
    // could fall within the actual sampling window, with rounding slack.
    if age_b <= whole_window.as_secs().saturating_add(1) {
        return Err("reset_continuity_indeterminate");
    }
    if a.errors
        .iter()
        .zip(b.errors)
        .any(|(x, y)| matches!((x,y),(Ok(x),Ok(y)) if *x > y))
    {
        return Err("counter_decreased");
    }
    Ok(std::array::from_fn(|i| match (a.errors[i], b.errors[i]) {
        (Ok(x), Ok(y)) => Ok(y - x),
        (Err(x), Err(y)) if x == y => Err(x),
        _ => Err("observation_state_changed"),
    }))
}

fn location_name(name: &str) -> bool {
    name.strip_prefix("dimm")
        .or_else(|| name.strip_prefix("rank"))
        .map_or(false, valid_suffix)
}

fn location_detail(dir: &Path, name: &str, initial: &Snapshot) -> String {
    let before = counter(&dir.join("seconds_since_reset"));
    let unknown = ["ce_noinfo_count", "ue_noinfo_count"].map(|key| counter(&dir.join(key)));
    let member = dir.join(name);
    let row = match identity(&member) {
        Ok(id) => {
            let counts = ["dimm_ce_count", "dimm_ue_count"].map(|key| counter(&member.join(key)));
            if identity(&member).ok() == Some(id) {
                format!(
                    r#"{{"status":"observed","counters":{}}}"#,
                    render_error_values(&counts)
                )
            } else {
                r#"{"status":"location_changed"}"#.into()
            }
        }
        Err(value) => format!(r#"{{"status":"{value}"}}"#),
    };
    let after = counter(&dir.join("seconds_since_reset"));
    let stable = matches!((&initial.reset_age, before, after), (Ok(a),Ok(b),Ok(c)) if *a <= b && b <= c)
        || matches!((&initial.reset_age, before, after), (Err(a),Err(b),Err(c)) if *a == b && b == c);
    if identity(dir).ok() != Some(initial.identity) || !stable {
        return format!(r#"{{"location":"{name}","status":"observation_changed"}}"#);
    }
    format!(
        r#"{{"location":"{name}","status":"observed","accounting":"details_not_additive_to_controller_total","unknown_location":{},"selected":{row}}}"#,
        render_error_values(&unknown)
    )
}

pub(crate) fn report(name: &str, wait_ms: Option<u64>) -> Result<String, String> {
    report_for(name, wait_ms, None)
}

fn report_for(name: &str, wait_ms: Option<u64>, location: Option<&str>) -> Result<String, String> {
    if !controller(name) {
        return Err("expected an EDAC controller such as mc0".into());
    }
    if let Some(location) = location {
        if !location_name(location) || wait_ms.is_some() {
            return Err(
                "location must be one DIMM or rank name and cannot be combined with sampling"
                    .into(),
            );
        }
    }
    let dir = Path::new(ROOT).join(name);
    let start = Instant::now();
    let first = snapshot(&dir);
    let result = if let Some(ms) = wait_ms {
        std::thread::sleep(Duration::from_millis(ms));
        let second = snapshot(&dir);
        let elapsed = start.elapsed();
        let values = match (&first, &second) {
            (Ok(a), Ok(b)) => deltas(a, b, elapsed),
            _ => Err("controller_observation_unavailable"),
        };
        let delta = match values {
            Ok(values) => format!(
                r#"{{"status":"compared","counters":{}}}"#,
                render_error_values(&values)
            ),
            Err(reason) => format!(r#"{{"status":"not_comparable","reason":"{reason}"}}"#),
        };
        format!(
            r#"{{"requested_wait_ms":{ms},"observation_window_ms":{},"before":{},"after":{},"delta":{delta}}}"#,
            elapsed.as_millis(),
            render(&first),
            render(&second)
        )
    } else {
        render(&first)
    };
    let locations = location.map_or_else(String::new, |name| {
        let detail = match &first {
            Ok(first) => location_detail(&dir, name, first),
            Err(reason) => format!(r#"{{"location":"{name}","status":"{reason}"}}"#),
        };
        format!(r#","location_detail":{detail}"#)
    });
    Ok(format!(
        r#"{{"schema_version":1,"kind":"linux_edac_memory","controller":"{name}","source":"linux_sysfs","result":{result}{locations}}}"#
    ))
}

pub(crate) fn run(name: &str, wait_ms: Option<u64>, location: Option<&str>) -> Result<(), String> {
    println!("{}", report_for(name, wait_ms, location)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_missing_and_zero_and_refuses_reset_or_decrease() {
        let make = |id, age, ce| Snapshot {
            identity: (1, id),
            reset_age: age,
            errors: [ce, Err("unavailable")],
        };
        let a = make(12, Ok(100), Ok(1));
        let window = Duration::from_millis(150);
        assert_eq!(
            deltas(&a, &make(12, Ok(100), Ok(1)), window),
            Ok([Ok(0), Err("unavailable")])
        );
        assert_eq!(
            deltas(&a, &make(12, Ok(100), Ok(0)), window),
            Err("counter_decreased")
        );
        assert_eq!(
            deltas(&a, &make(12, Ok(1), Ok(3)), window),
            Err("reset_age_decreased")
        );
        assert_eq!(
            deltas(&a, &make(13, Ok(100), Ok(3)), window),
            Err("controller_changed")
        );
        assert_eq!(
            deltas(&a, &make(12, Err("unavailable"), Ok(3)), window),
            Err("reset_age_unavailable")
        );
        let near = make(12, Ok(1), Ok(0));
        assert_eq!(
            deltas(&near, &near, window),
            Err("reset_continuity_indeterminate")
        );
    }

    #[test]
    fn location_is_detail_not_increment_to_controller_total() {
        let dir = std::env::temp_dir().join(format!("eld-edac-location-{}", std::process::id()));
        let dimm = dir.join("dimm0");
        fs::create_dir_all(&dimm).unwrap();
        fs::write(dir.join("ce_count"), "5\n").unwrap();
        fs::write(dir.join("ce_noinfo_count"), "2\n").unwrap();
        fs::write(dimm.join("dimm_ce_count"), "3\n").unwrap();
        let totals = snapshot(&dir).unwrap();
        assert_eq!(totals.errors[0], Ok(5));
        let out = location_detail(&dir, "dimm0", &totals);
        assert!(out.contains(r#""unknown_location":{"ce_count":{"status":"observed","value":"2"}"#));
        assert!(out.contains(r#""selected":{"status":"observed","counters":{"ce_count":{"status":"observed","value":"3"}"#));
        assert!(!out.contains(r#""value":"5""#));
        assert!(location_detail(&dir, "rank1", &totals)
            .contains(r#""selected":{"status":"unavailable"}"#));
        assert!(location_name("rank1") && !location_name("../dimm0"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn captures_only_selected_files_and_rejects_aliases() {
        let dir = std::env::temp_dir().join(format!("eld-edac-sampling-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("ce_count"), "18446744073709551615\n").unwrap();
        fs::write(dir.join("ue_count"), "oops\n").unwrap();
        fs::write(dir.join("reset_counters"), "unchanged\n").unwrap();
        let got = snapshot(&dir).unwrap();
        assert_eq!(got.errors, [Ok(u64::MAX), Err("invalid")]);
        assert_eq!(got.reset_age, Err("unavailable"));
        assert_eq!(
            fs::read_to_string(dir.join("reset_counters")).unwrap(),
            "unchanged\n"
        );
        std::os::unix::fs::symlink(&dir, dir.join("link")).unwrap();
        assert_eq!(snapshot(&dir.join("link")).err(), Some("invalid"));
        fs::remove_dir_all(&dir).unwrap();
    }
}

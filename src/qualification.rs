//! Inspect built-in qualification plans. Nothing here starts a workload.
use crate::pcie_health::json_string;

pub(crate) fn render(profile: &str, include_load: bool) -> Result<String, String> {
    let (seconds, ram_kib) = match profile {
        "bring-up" => (0u64, 0),
        "quick" => (1, 1024),
        "manufacturing" => (30, 4096),
        "overnight" => (4 * 3600, 8192),
        _ => return Err("expected one of: bring-up, quick, manufacturing, overnight".into()),
    };
    if include_load && seconds == 0 {
        return Err("the bring-up profile is passive only".into());
    }
    let mut steps = vec![
        r#"{"id":"inventory","status":"planned","kind":"passive","argv":["inventory"]}"#.to_owned(),
        r#"{"id":"board_identity_and_required_pcie","status":"manual_input_required","kind":"passive","reason":"requires an exact board profile chosen by the operator"}"#.to_owned(),
    ];
    for (name, unit) in [("cpu", "cpu"), ("ram", "ram")] {
        if include_load {
            let memory = if name == "ram" {
                format!(r#","--buffer-kib","{ram_kib}""#)
            } else {
                String::new()
            };
            steps.push(format!(
                r#"{{"id":"{name}_load","status":"planned","kind":"local_load","duration_target_seconds":{seconds},"argv":["stress","auto","--test","{unit}","--duration-secs","{seconds}"{memory}]}}"#
            ));
        } else {
            let reason = if seconds == 0 {
                "passive_profile"
            } else {
                "requires_explicit_local_load_preview"
            };
            steps.push(format!(
                r#"{{"id":"{name}_load","status":"skipped","kind":"local_load","reason":"{reason}"}}"#
            ));
        }
    }
    for (id, reason) in [
        (
            "storage_write",
            "requires an operator-selected scratch path and write budget",
        ),
        (
            "network_peer",
            "requires a dedicated peer and target selection",
        ),
        (
            "peripheral_and_accelerator",
            "requires explicit fixture, device and workload selection",
        ),
    ] {
        steps.push(format!(
            r#"{{"id":"{id}","status":"skipped","reason":{}}}"#,
            json_string(reason)
        ));
    }
    Ok(format!(
        r#"{{"plan_schema_version":1,"kind":"qualification_plan","profile":{},"execution":"none","hardware_availability":"not_checked","local_load_target_seconds":{},"steps":[{}]}}"#,
        json_string(profile),
        if include_load { seconds * 2 } else { 0 },
        steps.join(",")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_preview_never_includes_load_commands() {
        for profile in ["bring-up", "quick", "manufacturing", "overnight"] {
            let plan = render(profile, false).unwrap();
            assert!(plan.contains(r#""local_load_target_seconds":0"#));
            assert!(plan.contains(r#""execution":"none""#));
            assert!(!plan.contains(r#""argv":["stress""#));
        }
        assert!(render("bring-up", true).is_err());
        assert!(render("../../invalid", true).is_err());
    }

    #[test]
    fn longest_preview_has_a_fixed_budget_and_no_target_device_commands() {
        let plan = render("overnight", true).unwrap();
        assert!(plan.contains(r#""local_load_target_seconds":28800"#));
        assert_eq!(
            plan.matches(r#""argv":["stress","auto","--test""#).count(),
            2
        );
        assert!(plan.contains(r#""--buffer-kib","8192""#));
        assert!(!plan.contains(r#""--storage-dir""#));
        assert!(!plan.contains(r#""--device""#));
    }
}

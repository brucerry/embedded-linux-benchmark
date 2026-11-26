//! Inspect built-in qualification plans. Nothing here starts a workload.
use crate::pcie_health::json_string;

fn parameters(profile: &str) -> Result<(u64, u64), String> {
    Ok(match profile {
        "bring-up" => (0u64, 0),
        "quick" => (1, 1024),
        "manufacturing" => (30, 4096),
        "overnight" => (4 * 3600, 8192),
        _ => return Err("expected one of: bring-up, quick, manufacturing, overnight".into()),
    })
}

type PlannedWorkload = (&'static str, u64, Vec<String>);

pub(crate) fn planned_load(
    profile: &str,
    include_load: bool,
) -> Result<Vec<PlannedWorkload>, String> {
    let (seconds, ram_kib) = parameters(profile)?;
    if include_load && seconds == 0 {
        return Err("the bring-up profile is passive only".into());
    }
    if !include_load {
        return Ok(Vec::new());
    }
    Ok(["cpu", "ram"]
        .into_iter()
        .map(|workload| {
            let mut args = ["stress", "auto", "--test", workload, "--duration-secs"]
                .map(str::to_owned)
                .to_vec();
            args.push(seconds.to_string());
            if workload == "ram" {
                args.extend(["--buffer-kib".to_owned(), ram_kib.to_string()]);
            }
            (workload, seconds, args)
        })
        .collect())
}

pub(crate) fn render(profile: &str, include_load: bool) -> Result<String, String> {
    let load = planned_load(profile, include_load)?;
    let total_seconds = load.iter().map(|(_, seconds, _)| seconds).sum::<u64>();
    let mut steps = vec![
        r#"{"id":"inventory","status":"planned","kind":"passive","argv":["inventory"]}"#.to_owned(),
        r#"{"id":"board_identity_and_required_pcie","status":"manual_input_required","kind":"passive","reason":"requires an exact board profile chosen by the operator"}"#.to_owned(),
    ];
    for name in ["cpu", "ram"] {
        if let Some((_, seconds, args)) = load.iter().find(|row| row.0 == name) {
            let args = args
                .iter()
                .map(|v| json_string(v))
                .collect::<Vec<_>>()
                .join(",");
            steps.push(format!(
                r#"{{"id":"{name}_load","status":"planned","kind":"local_load","duration_target_seconds":{seconds},"argv":[{args}]}}"#
            ));
        } else {
            let reason = if profile == "bring-up" {
                "passive_profile"
            } else {
                "requires_explicit_local_load_preview"
            };
            steps.push(format!(
                r#"{{"id":"{name}_load","status":"skipped","kind":"local_load","reason":"{reason}"}}"#
            ));
        }
    }
    for (id, kind, reason) in [
        (
            "storage_write",
            "storage_write",
            "requires an operator-selected scratch path and write budget",
        ),
        (
            "network_peer",
            "network_peer",
            "requires a dedicated peer and target selection",
        ),
        (
            "peripheral_and_accelerator",
            "device_fixture",
            "requires explicit fixture, device and workload selection",
        ),
    ] {
        steps.push(format!(
            r#"{{"id":"{id}","status":"skipped","kind":"{kind}","reason":{}}}"#,
            json_string(reason)
        ));
    }
    Ok(format!(
        r#"{{"plan_schema_version":1,"kind":"qualification_plan","profile":{},"execution":"none","hardware_availability":"not_checked","local_load_target_seconds":{},"steps":[{}]}}"#,
        json_string(profile),
        total_seconds,
        steps.join(",")
    ))
}

/// Read inventory and, if requested, one passive NIC snapshot. No workload argv.
pub(crate) fn passive(
    board_profile: Option<&std::path::Path>,
    network_interface: Option<&str>,
) -> Result<String, String> {
    let network = network_interface
        .map(crate::network_health::snapshot)
        .transpose()?
        .map_or_else(String::new, |value| format!(r#","network_health":{value}"#));
    let inventory = crate::inventory::snapshot(None, board_profile, None)?;
    let board_check = if board_profile.is_some() {
        "see_inventory"
    } else {
        "not_configured"
    };
    Ok(format!(
        r#"{{"qualification_schema_version":1,"kind":"passive_qualification_observation","scope":"bring-up","workload_execution":"none","board_check":"{board_check}","inventory":{inventory}{network}}}"#
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

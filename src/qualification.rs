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

pub(crate) fn render(
    profile: &str,
    include_load: bool,
    local_test: Option<&str>,
    budget_seconds: Option<u64>,
) -> Result<String, String> {
    if !include_load && (local_test.is_some() || budget_seconds.is_some()) {
        return Err("local-test and max-local-load-secs require --include-local-load".into());
    }
    if !matches!(local_test, None | Some("cpu" | "ram")) {
        return Err("local-test must be cpu or ram".into());
    }
    let mut load = planned_load(profile, include_load)?;
    if let Some(selected) = local_test {
        load.retain(|(name, _, _)| *name == selected);
    }
    let total_seconds = load.iter().map(|(_, seconds, _)| seconds).sum::<u64>();
    let budget = match budget_seconds {
        Some(limit) if total_seconds > limit => {
            return Err(format!(
                "planned local load ({total_seconds}s) exceeds the supplied budget ({limit}s)"
            ));
        }
        Some(limit) => format!(r#","local_load_budget_seconds":{limit}"#),
        None => String::new(),
    };
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
            } else if local_test.is_some() {
                "not_selected"
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
        r#"{{"plan_schema_version":1,"kind":"qualification_plan","profile":{},"execution":"none","hardware_availability":"not_checked","local_load_target_seconds":{}{budget},"steps":[{}]}}"#,
        json_string(profile),
        total_seconds,
        steps.join(",")
    ))
}

pub(crate) enum BoardScope<'a> {
    None,
    Profile(&'a std::path::Path),
    RequiredPcie(&'a str),
}

pub(crate) enum SystemHealthScope {
    None,
    Snapshot,
    WithPressureSeries { count: u64, gap_ms: u64 },
}

pub(crate) struct HealthOptions {
    pub system: SystemHealthScope,
    pub thermal_wait_ms: Option<u64>,
    pub include_timeline: bool,
}

struct PassiveTimeline {
    start: Option<std::time::Instant>,
    rows: Vec<String>,
}

impl PassiveTimeline {
    fn new(enabled: bool) -> Self {
        Self {
            start: enabled.then(std::time::Instant::now),
            rows: Vec::new(),
        }
    }

    fn observe<T>(&mut self, phase: &'static str, read: impl FnOnce() -> T) -> T {
        let Some(origin) = self.start else {
            return read();
        };
        let begin = origin.elapsed().as_millis();
        let observation = read();
        let end = origin.elapsed().as_millis();
        self.rows.push(format!(
            r#"{{"phase":"{phase}","started_monotonic_ms":{begin},"finished_monotonic_ms":{end}}}"#,
        ));
        observation
    }

    fn render(&self) -> String {
        if self.start.is_none() {
            return String::new();
        }
        format!(
            r#","observation_timeline":{{"schema_version":1,"clock":"single_process_monotonic","execution":"sequential","phases":[{}]}}"#,
            self.rows.join(",")
        )
    }
}

/// Read inventory and selected passive snapshots. No workload argv.
fn passive(
    board: BoardScope<'_>,
    network_interface: Option<(&str, Option<u64>)>,
    nvme_controller: Option<&str>,
    run_id: Option<&str>,
    health_options: HealthOptions,
    pcie: Option<(&str, Option<u64>)>,
) -> Result<String, String> {
    let run = match run_id {
        Some(id) if valid_run_id(id) => format!(r#","run_id":{}"#, json_string(id)),
        Some(_) => {
            return Err(
                "run-id must be exactly 32 lowercase hexadecimal characters and not all zero"
                    .into(),
            )
        }
        None => String::new(),
    };
    let mut timeline = PassiveTimeline::new(health_options.include_timeline);
    let network = network_interface
        .map(|(name, interval)| {
            timeline.observe("network_health", || {
                crate::network_health::render(name, interval)
            })
        })
        .transpose()?
        .map_or_else(String::new, |value| format!(r#","network_health":{value}"#));
    let nvme = nvme_controller
        .map(|name| timeline.observe("nvme_health", || crate::nvme_health::snapshot(name)))
        .transpose()?
        .map_or_else(String::new, |value| format!(r#","nvme_health":{value}"#));
    let pcie = pcie
        .map(|(device, ms)| {
            timeline.observe("pcie_health", || {
                crate::pcie_health::render_snapshot(device, ms, Default::default())
            })
        })
        .transpose()?
        .map_or_else(String::new, |value| format!(r#","pcie_health":{value}"#));
    let (required, profile, board_check) = match board {
        BoardScope::None => (None, None, "not_configured"),
        BoardScope::Profile(path) => (None, Some(path), "see_inventory"),
        BoardScope::RequiredPcie(expected) => (Some(expected), None, "see_required_pcie"),
    };
    let inventory = timeline.observe("inventory", || {
        crate::inventory::snapshot(required, profile, health_options.thermal_wait_ms)
    })?;
    let health = match health_options.system {
        SystemHealthScope::None => String::new(),
        _ => timeline.observe("system_health", || -> Result<String, String> {
            let numa = crate::numa::snapshot();
            let pressure = crate::pressure::snapshot();
            let series = if let SystemHealthScope::WithPressureSeries { count, gap_ms } =
                health_options.system
            {
                format!(
                    r#","pressure_series":{}"#,
                    crate::pressure::series(count, gap_ms)?
                )
            } else {
                String::new()
            };
            Ok(format!(
                r#","system_health":{{"schema_version":1,"numa":{numa},"pressure":{pressure}{series}}}"#,
            ))
        })?,
    };
    let timeline = timeline.render();
    Ok(format!(
        r#"{{"qualification_schema_version":1,"kind":"passive_qualification_observation","scope":"bring-up","workload_execution":"none","board_check":"{board_check}","inventory":{inventory}{network}{nvme}{health}{pcie}{run}{timeline}}}"#
    ))
}

fn valid_run_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        && value.bytes().any(|b| b != b'0')
}

pub(crate) const MAX_EVIDENCE_BYTES: usize = 1024 * 1024;

pub(crate) fn run_passive(
    board: BoardScope<'_>,
    network_interface: Option<(&str, Option<u64>)>,
    nvme_controller: Option<&str>,
    evidence_path: Option<&std::path::Path>,
    run_id: Option<&str>,
    health_options: HealthOptions,
    pcie: Option<(&str, Option<u64>)>,
) -> Result<(), String> {
    use crate::common::{unix_ms, write_new_evidence};
    use std::time::{Instant, SystemTime};
    let clock = evidence_path.map(|_| (SystemTime::now(), Instant::now()));
    let observation = passive(
        board,
        network_interface,
        nvme_controller,
        run_id,
        health_options,
        pcie,
    )?;
    if let Some((path, (started, timer))) = evidence_path.zip(clock) {
        let elapsed = timer.elapsed().as_millis();
        let finished = unix_ms(SystemTime::now());
        let evidence = format!(
            concat!(
                r#"{{"evidence_schema_version":1,"kind":"passive_qualification","started_unix_ms":{},"finished_unix_ms":{},"elapsed_monotonic_ms":{},"qualification":{}}}"#,
                "\n"
            ),
            unix_ms(started),
            finished,
            elapsed,
            observation
        );
        if evidence.len() > MAX_EVIDENCE_BYTES {
            return Err("qualification evidence exceeds 1 MiB; no file created".to_owned());
        }
        write_new_evidence(path, evidence.as_bytes())
            .map_err(|e| format!("cannot save qualification evidence (choose a new path): {e}"))?;
    }
    println!("{observation}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passive_preview_never_includes_load_commands() {
        for profile in ["bring-up", "quick", "manufacturing", "overnight"] {
            let plan = render(profile, false, None, None).unwrap();
            assert!(plan.contains(r#""local_load_target_seconds":0"#));
            assert!(plan.contains(r#""execution":"none""#));
            assert!(!plan.contains(r#""argv":["stress""#));
        }
        assert!(render("bring-up", true, None, None).is_err());
        assert!(render("../../invalid", true, None, None).is_err());
    }

    #[test]
    fn longest_preview_has_a_fixed_budget_and_no_target_device_commands() {
        let plan = render("overnight", true, None, None).unwrap();
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

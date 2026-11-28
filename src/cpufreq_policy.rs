//! Read one selected cpufreq policy without setting a governor or frequency.
use crate::sysfs::{attribute_status, bounded_text};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

fn report(root: &Path, policy: &str) -> Result<String, String> {
    let Some(index) = policy.strip_prefix("policy") else {
        return Err("expected a canonical policyN selector".into());
    };
    if index.is_empty()
        || index.len() > 7
        || (index.len() > 1 && index.starts_with('0'))
        || !index.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("expected a canonical policyN selector".into());
    }
    let selected = root.join(policy);
    let identity = || {
        let m = fs::symlink_metadata(&selected).map_err(attribute_status)?;
        if !m.is_dir() {
            return Err("invalid");
        }
        Ok((m.dev(), m.ino()))
    };
    let withheld = |state| {
        format!(
            r#"{{"schema_version":1,"kind":"cpufreq_policy","policy":"{policy}","status":"{state}"}}"#
        )
    };
    let initial = match identity() {
        Ok(id) => id,
        Err(state) => return Ok(withheld(state)),
    };
    let items = [
        "scaling_min_freq",
        "scaling_max_freq",
        "scaling_cur_freq",
        "cpuinfo_cur_freq",
    ];
    let fields = items
        .iter()
        .map(|key| {
            let reading = bounded_text(&selected.join(key)).and_then(|raw| {
                if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("invalid");
                }
                raw.parse::<u64>().ok().filter(|n| *n > 0).ok_or("invalid")
            });
            let value = match reading {
                Ok(n) => format!(r#"{{"status":"observed","value":"{n}"}}"#),
                Err(status) => format!(r#"{{"status":"{status}"}}"#),
            };
            format!(r#""{key}":{value}"#)
        })
        .collect::<Vec<_>>()
        .join(",");
    if identity().ok() != Some(initial) {
        return Ok(withheld("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"kind":"cpufreq_policy","policy":"{policy}","status":"observed","unit":"kHz","fields":{{{fields}}}}}"#
    ))
}

pub(crate) fn run(policy: &str) -> Result<(), String> {
    println!(
        "{}",
        report(Path::new("/sys/devices/system/cpu/cpufreq"), policy)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn keeps_scaling_and_hardware_observations_separate() {
        let root =
            std::env::temp_dir().join(format!("eld-selected-cpufreq-{}", std::process::id()));
        let policy = root.join("policy0");
        fs::create_dir_all(&policy).unwrap();
        fs::write(policy.join("scaling_min_freq"), "400000\n").unwrap();
        fs::write(policy.join("scaling_max_freq"), "2500000\n").unwrap();
        fs::write(policy.join("scaling_cur_freq"), "1200000\n").unwrap();
        let out = report(&root, "policy0").unwrap();
        assert!(out.contains(r#""unit":"kHz""#));
        assert!(out.contains(r#""scaling_cur_freq":{"status":"observed","value":"1200000"}"#));
        assert!(out.contains(r#""cpuinfo_cur_freq":{"status":"unavailable"}"#));
        symlink(
            policy.join("scaling_cur_freq"),
            policy.join("cpuinfo_cur_freq"),
        )
        .unwrap();
        assert!(report(&root, "policy0")
            .unwrap()
            .contains(r#""cpuinfo_cur_freq":{"status":"invalid"}"#));
        assert!(report(&root, "policy1")
            .unwrap()
            .contains(r#""status":"unavailable""#));
        assert!(report(&root, "policy00").is_err());
        assert!(report(&root, "../policy0").is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

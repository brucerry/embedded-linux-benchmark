use crate::sysfs::{attribute_status, name_digits, open_attribute};
use std::{io::Read, path::Path};

const MAX_NODES: usize = 64;
const MAX_CPUS: usize = 8192;
type Reading<T> = Result<T, &'static str>;

fn read(path: &Path, limit: usize) -> Reading<String> {
    let mut text = String::new();
    open_attribute(path)
        .map_err(attribute_status)?
        .take((limit + 1) as u64)
        .read_to_string(&mut text)
        .map_err(attribute_status)?;
    (text.len() <= limit).then_some(text).ok_or("invalid")
}

fn ids(text: &str, max: usize, allow_empty: bool) -> Reading<Vec<u32>> {
    let mut values = Vec::new();
    let text = text.trim();
    if text.is_empty() && allow_empty {
        return Ok(values);
    }
    for token in text.split(',') {
        let number = |s: &str| {
            if !name_digits(s) {
                return Err("invalid");
            }
            s.parse::<u32>().map_err(|_| "invalid")
        };
        let (first, last) = if let Some((a, b)) = token.split_once('-') {
            (number(a)?, number(b)?)
        } else {
            let n = number(token)?;
            (n, n)
        };
        if first > last || values.last().map_or(false, |n| *n >= first) {
            return Err("invalid");
        }
        if u64::from(last) - u64::from(first) + 1 > (max - values.len()) as u64 {
            return Err("invalid");
        }
        values.extend(first..=last);
    }
    Ok(values)
}

fn memory(data: &str, node: u32) -> Reading<u64> {
    let mut total = None;
    for line in data.lines() {
        let mut f = line.split_ascii_whitespace();
        if let (Some("Node"), Some(id), Some("MemTotal:")) = (f.next(), f.next(), f.next()) {
            if total.is_some() || id.parse::<u32>().ok() != Some(node) {
                return Err("invalid");
            }
            let value = f.next().filter(|s| name_digits(s)).ok_or("invalid")?;
            if f.next() != Some("kB") || f.next().is_some() {
                return Err("invalid");
            }
            total = value.parse::<u64>().ok().and_then(|n| n.checked_mul(1024));
            if total.is_none() {
                return Err("invalid");
            }
        }
    }
    total.ok_or("invalid")
}

fn result<T>(value: Reading<T>, render: impl FnOnce(T) -> String) -> String {
    match value {
        Ok(v) => format!(r#"{{"status":"observed","value":{}}}"#, render(v)),
        Err(s) => format!(r#"{{"status":"{s}"}}"#),
    }
}

fn observe(root: &Path) -> Reading<Vec<String>> {
    let online = root.join("online");
    let before = ids(&read(&online, 4096)?, MAX_NODES, false)?;
    let mut rows = Vec::with_capacity(before.len());
    let mut observed_cpus = 0;
    for node in &before {
        let dir = root.join(format!("node{node}"));
        let cpu = read(&dir.join("cpulist"), 8192).and_then(|s| ids(&s, MAX_CPUS, true));
        if let Ok(ref list) = cpu {
            observed_cpus += list.len();
            if observed_cpus > MAX_CPUS {
                return Err("limit_exceeded");
            }
        }
        let mem = read(&dir.join("meminfo"), 16384).and_then(|s| memory(&s, *node));
        rows.push(format!(
            r#"{{"node_id":{node},"cpu_ids":{},"memory_total_bytes":{}}}"#,
            result(cpu, |v| format!("{v:?}")),
            result(mem, |v| format!("\"{v}\""))
        ));
    }
    if ids(&read(&online, 4096)?, MAX_NODES, false)? != before {
        return Err("changed_during_read");
    }
    Ok(rows)
}

pub(crate) fn snapshot() -> String {
    format!(
        r#"{{"schema_version":1,"scope":"linux_numa_inventory","nodes":{}}}"#,
        result(observe(Path::new("/sys/devices/system/node")), |v| format!(
            "[{}]",
            v.join(",")
        ))
    )
}

pub(crate) fn run() {
    println!("{}", snapshot());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_lists_keep_sparse_ids_and_bound_expansion() {
        assert_eq!(
            ids("0,2-3,2147483649\n", 4, false).unwrap(),
            [0, 2, 3, 2147483649]
        );
        assert!(ids("0-4294967295", MAX_CPUS, true).is_err());
        assert!(ids("0-3,3-4", MAX_CPUS, true).is_err());
        assert!(ids("2,1", MAX_CPUS, false).is_err());
        assert!(ids("0, 1", MAX_CPUS, false).is_err());
        assert_eq!(ids("\n", MAX_CPUS, true).unwrap(), []);
        assert!(ids("\n", MAX_NODES, false).is_err());
    }
    #[test]
    fn memory_totals_preserve_zero_and_check_units_and_node() {
        assert_eq!(memory("Node 5 MemTotal:   0 kB\n", 5), Ok(0));
        assert_eq!(memory("Node 5 MemTotal:   256 kB\n", 5), Ok(262144));
        assert!(memory("Node 7 MemTotal:   0 KB", 5).is_err());
        assert!(memory("Node 7 MemTotal:   0 kB", 5).is_err());
        assert!(memory("Node 5 MemTotal:   18446744073709551615 kB", 5).is_err());
    }
    #[test]
    fn observes_sparse_nodes_without_fabricating_missing_metrics() {
        use std::fs::{create_dir, remove_dir_all, write};
        let root = std::env::temp_dir().join(format!("eld-numa-fixture-{}", std::process::id()));
        create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        write(root.join("online"), "0,8").unwrap();
        create_dir(root.join("node0")).unwrap();
        create_dir(root.join("node8")).unwrap();
        write(root.join("node0/cpulist"), "0-3").unwrap();
        write(root.join("node0/meminfo"), "Node 0 MemTotal: 1 kB").unwrap();
        write(root.join("node8/cpulist"), "\n").unwrap();
        let rows = observe(&root).unwrap();
        assert!(rows[0].contains(r#""value":"1024""#));
        assert!(rows[1].contains(r#""node_id":8"#));
        assert!(rows[1].contains(r#""status":"observed","value":[]"#));
        assert!(rows[1].contains(r#""memory_total_bytes":{"status":"unavailable"}"#));
        write(root.join("node8/meminfo"), "Node 0 MemTotal: 1 kB").unwrap();
        assert!(observe(&root).unwrap()[1].contains(r#""memory_total_bytes":{"status":"invalid"}"#));
        write(root.join("node0/cpulist"), "0-8191").unwrap();
        write(root.join("node8/cpulist"), "8192").unwrap();
        assert_eq!(observe(&root).unwrap_err(), "limit_exceeded");
    }
}

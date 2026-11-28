//! Read fixed counters from one explicitly selected HugeTLB pool; no allocations.
use crate::sysfs::{attribute_status, numeric_json};
use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub(crate) const MAX_SIZE_KIB: u64 = 1 << 30;

fn report(root: &Path, size_kib: u64) -> Result<String, String> {
    if !(1..=MAX_SIZE_KIB).contains(&size_kib) || !size_kib.is_power_of_two() {
        return Err("expected a positive power-of-two HugeTLB page size in KiB".to_owned());
    }
    let path = root.join(format!("hugepages-{size_kib}kB"));
    let identity = || {
        let info = fs::symlink_metadata(&path).map_err(attribute_status)?;
        if !info.is_dir() {
            return Err("invalid");
        }
        Ok((info.dev(), info.ino()))
    };
    let unavailable = |status| {
        format!(
            r#"{{"schema_version":1,"kind":"hugetlb_pool","size_kib":{size_kib},"status":"{status}"}}"#,
        )
    };
    let initial = match identity() {
        Ok(value) => value,
        Err(status) => return Ok(unavailable(status)),
    };
    let fields = [
        "nr_hugepages",
        "free_hugepages",
        "resv_hugepages",
        "surplus_hugepages",
    ]
    .iter()
    .map(|key| format!(r#""{key}":{}"#, numeric_json(&path.join(key), false)))
    .collect::<Vec<_>>()
    .join(",");
    if identity().ok() != Some(initial) {
        return Ok(unavailable("changed"));
    }
    Ok(format!(
        r#"{{"schema_version":1,"kind":"hugetlb_pool","size_kib":{size_kib},"status":"observed","counters":{{{fields}}}}}"#,
    ))
}

pub(crate) fn run(size_kib: u64) -> Result<(), String> {
    println!(
        "{}",
        report(Path::new("/sys/kernel/mm/hugepages"), size_kib)?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    #[test]
    fn only_reads_selected_pool_and_preserves_zero_and_missing() {
        let root = std::env::temp_dir().join(format!("eld-hugetlb-{}", std::process::id()));
        let pool = root.join("hugepages-2048kB");
        fs::create_dir_all(&pool).unwrap();
        fs::write(pool.join("nr_hugepages"), "4\n").unwrap();
        fs::write(pool.join("free_hugepages"), "0\n").unwrap();
        fs::write(pool.join("surplus_hugepages"), "-1\n").unwrap();
        let data = report(&root, 2048).unwrap();
        assert!(data.contains(r#""free_hugepages":{"status":"observed","value":"0"}"#));
        assert!(data.contains(r#""resv_hugepages":{"status":"unavailable"}"#));
        assert!(data.contains(r#""surplus_hugepages":{"status":"invalid"}"#));
        assert!(report(&root, 1024)
            .unwrap()
            .contains(r#""status":"unavailable""#));
        assert!(report(&root, 2049).is_err());
        symlink(&pool, root.join("hugepages-4096kB")).unwrap();
        assert!(report(&root, 4096)
            .unwrap()
            .contains(r#""status":"invalid""#));
        fs::remove_dir_all(root).unwrap();
    }
}

use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_embedded-linux-benchmark"))
        .args(args)
        .output()
        .expect("launch benchmark CLI")
}

fn successful_output(args: &[&str]) -> String {
    let output = run(args);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    String::from_utf8(output.stdout).expect("UTF-8 stdout")
}

fn field<'a>(output: &'a str, label: &str) -> &'a str {
    output
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("missing '{label}' in output:\n{output}"))
}

fn rate(output: &str, unit: &str) -> f64 {
    field(output, "Throughput: ")
        .strip_suffix(unit)
        .expect("throughput unit")
        .parse()
        .expect("numeric throughput")
}

fn elapsed(output: &str) -> f64 {
    field(output, "Elapsed: ")
        .strip_suffix(" s")
        .expect("elapsed time unit")
        .parse()
        .expect("numeric elapsed time")
}

#[test]
fn help_lists_both_benchmarks() {
    for args in [vec![], vec!["--help"], vec!["ram", "-h"]] {
        let output = successful_output(&args);
        assert!(output.contains("embedded-linux-benchmark cpu"));
        assert!(output.contains("embedded-linux-benchmark ram"));
        assert!(output.contains("--buffer-kib"));
    }
}

#[test]
fn cpu_reports_checksum_for_its_counted_iterations() {
    let output = successful_output(&["cpu", "--duration-ms", "1"]);
    assert!(output.starts_with("CPU throughput (single thread, 32-bit xorshift)\n"));
    let iterations: u64 = field(&output, "Iterations: ").parse().unwrap();
    assert!(iterations > 0 && iterations % 4_096 == 0);
    assert!(elapsed(&output) >= 0.001);
    assert!(rate(&output, " iterations/s") > 0.0);

    let mut state = 0x1234_5678u32;
    for _ in 0..iterations {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
    }
    assert_eq!(field(&output, "Checksum: "), format!("{state:08x}"));
}

#[test]
fn ram_reports_both_buffers_and_counts_one_payload_per_copy() {
    for (value, kib) in [("4", 4), ("32768", 32_768)] {
        let output = successful_output(&["ram", "--buffer-kib", value, "--duration-ms", "1"]);
        assert!(output.starts_with("RAM copy throughput (single thread)\n"));
        assert_eq!(field(&output, "Buffer size: "), format!("{kib} KiB each"));
        assert_eq!(
            field(&output, "Working set: "),
            format!("{} KiB (source + destination)", kib * 2)
        );
        let copies: u64 = field(&output, "Copies: ").parse().unwrap();
        let bytes: u64 = field(&output, "Bytes copied: ").parse().unwrap();
        assert!(copies > 0);
        assert_eq!(bytes, copies * kib * 1_024);
        assert!(elapsed(&output) >= 0.001);
        assert!(rate(&output, " MiB/s") > 0.0);
    }
}

#[test]
fn invalid_inputs_fail_before_running_a_benchmark() {
    let cases: &[(&[&str], &str)] = &[
        (&["disk"], "unknown command 'disk'"),
        (
            &["cpu", "--buffer-kib", "4"],
            "unknown option '--buffer-kib'",
        ),
        (
            &["cpu", "--duration-ms"],
            "missing value for '--duration-ms'",
        ),
        (
            &["cpu", "--duration-ms", "0"],
            "integer between 1 and 60000",
        ),
        (
            &["cpu", "--duration-ms", "60001"],
            "integer between 1 and 60000",
        ),
        (
            &["cpu", "--duration-ms", "1.5"],
            "integer between 1 and 60000",
        ),
        (&["ram", "--buffer-kib", "3"], "integer between 4 and 32768"),
        (
            &["ram", "--buffer-kib", "32769"],
            "integer between 4 and 32768",
        ),
        (
            &["ram", "--buffer-kib", "18446744073709551616"],
            "integer between 4 and 32768",
        ),
        (
            &["ram", "--buffer-kib", "4", "--buffer-kib", "8"],
            "may only be set once",
        ),
    ];
    for (args, message) in cases {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(message),
            "{args:?}"
        );
    }
}

struct ScratchDirectory(std::path::PathBuf);

impl ScratchDirectory {
    fn create() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);

        loop {
            let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "embedded-linux-benchmark-cli-{}-{sequence}",
                std::process::id()
            ));
            match std::fs::create_dir(&directory) {
                Ok(()) => return Self(directory),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create test directory: {error}"),
            }
        }
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("UTF-8 test path")
    }

    fn names(&self) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for ScratchDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn storage_reports_buffered_and_synced_modes_and_removes_its_file() {
    let directory = ScratchDirectory::create();
    let sentinel = directory.0.join(".embedded-linux-benchmark-keep");
    std::fs::write(&sentinel, b"keep this file").unwrap();
    for (extra, mode) in [
        (None, "buffered, page cache eligible"),
        (Some("--sync"), "buffered, includes sync_data"),
    ] {
        let mut args = vec!["storage", "--dir", directory.path(), "--size-mib", "1"];
        args.extend(extra);
        let output = successful_output(&args);
        assert_eq!(field(&output, "Write mode: "), mode);
        assert_eq!(field(&output, "Write bytes: "), "1048576");
        assert_eq!(field(&output, "Read bytes: "), "1048576");
        assert!(field(&output, "Read mode: ").contains("page cache eligible"));
        assert_eq!(directory.names(), [".embedded-linux-benchmark-keep"]);
        assert_eq!(std::fs::read(&sentinel).unwrap(), b"keep this file");
    }
}

#[test]
fn loopback_receives_exactly_the_requested_bytes() {
    let output = successful_output(&["network", "--size-mib", "1"]);
    assert!(output.starts_with("Network throughput (TCP loopback, single thread)\n"));
    assert_eq!(field(&output, "Address: "), "127.0.0.1");
    assert_eq!(field(&output, "Bytes received: "), "1048576");
    assert!(rate(&output, " MiB/s") > 0.0);
}

#[test]
fn stress_runs_multiple_rounds_without_storage_and_honors_an_opt_in_budget() {
    let directory = ScratchDirectory::create();
    let args = [
        "stress",
        "auto",
        "--rounds",
        "3",
        "--duration-ms",
        "1",
        "--buffer-kib",
        "4",
        "--network-mib",
        "1",
    ];
    let result = Command::new(env!("CARGO_BIN_EXE_embedded-linux-benchmark"))
        .args(args)
        .current_dir(&directory.0)
        .output()
        .unwrap();
    assert!(
        result.status.success() && result.stderr.is_empty(),
        "{result:?}"
    );
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("Storage: disabled"));
    assert!(!output.contains("Storage throughput"));
    for component in ["CPU: PASS", "RAM: PASS", "Network: PASS"] {
        assert_eq!(output.lines().filter(|line| *line == component).count(), 3);
    }
    assert!(output.contains("Stress result: PASS (3/3 rounds completed)"));
    assert!(directory.names().is_empty());

    let mut args = args.to_vec();
    args.extend([
        "--storage-dir",
        directory.path(),
        "--storage-mib",
        "1",
        "--storage-sync",
    ]);
    let output = successful_output(&args);
    let written: Vec<u64> = output
        .lines()
        .filter_map(|line| line.strip_prefix("Write bytes: "))
        .map(|bytes| bytes.parse().unwrap())
        .collect();
    assert_eq!(written, [349525, 349525, 349526]);
    assert_eq!(
        output
            .lines()
            .filter(|line| *line == "Storage: PASS")
            .count(),
        3
    );
    assert!(output.contains("Write mode: buffered, includes sync_data"));
    assert!(output.contains("Stress result: PASS (3/3 rounds completed)"));
    assert!(directory.names().is_empty());
}

#[test]
fn stress_selector_runs_only_the_selected_component_each_round() {
    let directory = ScratchDirectory::create();
    let components = [
        ("CPU", "CPU throughput ("),
        ("RAM", "RAM copy throughput ("),
        ("Network", "Network throughput ("),
        ("Storage", "Storage throughput ("),
    ];
    let cases: &[(&str, &[&str])] = &[
        ("cpu", &["--duration-ms", "1"]),
        ("ram", &["--duration-ms", "1", "--buffer-kib", "4"]),
        ("network", &["--network-mib", "1"]),
        (
            "storage",
            &[
                "--storage-dir",
                directory.path(),
                "--storage-mib",
                "1",
                "--storage-sync",
            ],
        ),
    ];
    for (test, options) in cases {
        let mut args = vec!["stress", "auto", "--test", test, "--rounds", "3"];
        args.extend_from_slice(options);
        let output = successful_output(&args);
        assert!(output.contains("Stress result: PASS (3/3 rounds completed)"));
        for (component, throughput) in components {
            let expected = if component.eq_ignore_ascii_case(test) {
                3
            } else {
                0
            };
            let outcome = format!("{component}: PASS");
            assert_eq!(
                output.lines().filter(|line| *line == outcome).count(),
                expected,
                "{test}"
            );
            assert_eq!(
                output
                    .lines()
                    .filter(|line| line.starts_with(throughput))
                    .count(),
                expected,
                "{test}"
            );
        }
        if *test == "storage" {
            for label in ["Write bytes: ", "Read bytes: "] {
                let counts: Vec<&str> = output
                    .lines()
                    .filter_map(|line| line.strip_prefix(label))
                    .collect();
                assert_eq!(counts, ["349525", "349525", "349526"]);
            }
            assert!(output.contains("Write mode: buffered, includes sync_data"));
        }
        assert!(directory.names().is_empty(), "{test}");
    }
}

#[test]
fn stress_rejects_invalid_selectors_and_conflicting_options_before_work() {
    let directory = ScratchDirectory::create();
    let cases: &[(&[&str], &str)] = &[
        (&["--test", "all"], "invalid stress test 'all'"),
        (&["--test", "cpu,ram"], "invalid stress test 'cpu,ram'"),
        (&["--test"], "missing value for '--test'"),
        (&["--test", "cpu", "--test", "ram"], "may only be set once"),
        (&["--test", "storage"], "requires both"),
        (
            &["--test", "storage", "--storage-dir", directory.path()],
            "requires both",
        ),
        (
            &["--test", "storage", "--storage-mib", "1"],
            "requires both",
        ),
        (
            &[
                "--test",
                "storage",
                "--storage-dir",
                directory.path(),
                "--storage-mib",
                "65",
            ],
            "integer between 1 and 64",
        ),
        (
            &["--test", "ram", "--timeout-ms", "750"],
            "selected test's total phase duration",
        ),
        (
            &["--test", "network", "--duration-ms", "1"],
            "'--duration-ms' cannot be used",
        ),
        (
            &["--test", "cpu", "--buffer-kib", "4"],
            "'--buffer-kib' cannot be used",
        ),
        (
            &["--test", "ram", "--network-mib", "1"],
            "'--network-mib' cannot be used",
        ),
        (
            &[
                "--test",
                "cpu",
                "--storage-dir",
                directory.path(),
                "--storage-mib",
                "1",
            ],
            "'--storage-dir' cannot be used",
        ),
        (
            &["--test", "ram", "--storage-mib", "1"],
            "'--storage-mib' cannot be used",
        ),
        (
            &["--test", "network", "--storage-sync"],
            "'--storage-sync' cannot be used",
        ),
    ];
    for (options, expected) in cases {
        let mut args = vec!["stress", "auto"];
        args.extend_from_slice(options);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(expected),
            "{args:?}"
        );
    }
    assert!(directory.names().is_empty());
}

#[test]
fn category_parameters_are_validated_before_load_or_writes() {
    let directory = ScratchDirectory::create();
    let ordinary_file = directory.0.join("ordinary-file");
    std::fs::write(&ordinary_file, b"untouched").unwrap();
    let cases: &[(&[&str], &str)] = &[
        (
            &["storage", "--dir", directory.path()],
            "requires '--size-mib'",
        ),
        (&["storage", "--size-mib", "1"], "requires '--dir'"),
        (
            &["storage", "--dir", directory.path(), "--size-mib", "65"],
            "integer between 1 and 64",
        ),
        (
            &[
                "storage",
                "--dir",
                ordinary_file.to_str().unwrap(),
                "--size-mib",
                "1",
            ],
            "must be a directory",
        ),
        (&["network", "--size-mib", "0"], "integer between 1 and 64"),
        (
            &["network", "--timeout-ms", "30001"],
            "integer between 1 and 30000",
        ),
        (
            &["network", "--host", "192.0.2.1"],
            "unknown option '--host'",
        ),
        (&["stress"], "expected 'stress auto'"),
        (
            &["stress", "auto", "--rounds", "6"],
            "integer between 1 and 5",
        ),
        (
            &["stress", "auto", "--duration-ms", "5001"],
            "integer between 1 and 5000",
        ),
        (
            &["stress", "auto", "--timeout-ms", "1"],
            "combined CPU and RAM phase durations",
        ),
        (
            &["stress", "auto", "--storage-dir", directory.path()],
            "requires both",
        ),
        (&["stress", "auto", "--storage-mib", "1"], "requires both"),
        (&["stress", "auto", "--storage-sync"], "requires both"),
        (
            &[
                "stress",
                "auto",
                "--storage-dir",
                directory.path(),
                "--storage-mib",
                "65",
            ],
            "integer between 1 and 64",
        ),
        (
            &[
                "stress",
                "auto",
                "--storage-dir",
                ordinary_file.to_str().unwrap(),
                "--storage-mib",
                "1",
            ],
            "must be a directory",
        ),
    ];
    for (args, expected) in cases {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(expected),
            "{args:?}"
        );
    }
    assert_eq!(directory.names(), ["ordinary-file"]);
    assert_eq!(std::fs::read(ordinary_file).unwrap(), b"untouched");
}

#[cfg(target_os = "linux")]
#[test]
fn failed_writes_remove_scratch_files_and_stop_stress() {
    let directory = ScratchDirectory::create();
    for (args, stress) in [
        (
            vec!["storage", "--dir", directory.path(), "--size-mib", "1"],
            false,
        ),
        (
            vec![
                "stress",
                "auto",
                "--rounds",
                "2",
                "--duration-ms",
                "1",
                "--buffer-kib",
                "4",
                "--network-mib",
                "1",
                "--storage-dir",
                directory.path(),
                "--storage-mib",
                "1",
            ],
            true,
        ),
        (
            vec![
                "stress",
                "auto",
                "--test",
                "storage",
                "--rounds",
                "2",
                "--storage-dir",
                directory.path(),
                "--storage-mib",
                "1",
            ],
            true,
        ),
    ] {
        // Ignore SIGXFSZ so exceeding the child file-size limit returns a write error.
        let output = Command::new("/bin/sh")
            .args(["-c", "trap '' XFSZ && ulimit -f 1 && exec \"$@\"", "sh"])
            .arg(env!("CARGO_BIN_EXE_embedded-linux-benchmark"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(String::from_utf8(output.stderr)
            .unwrap()
            .contains("storage write failed"));
        let stdout = String::from_utf8(output.stdout).unwrap();
        if stress {
            assert!(stdout.contains("Storage: FAIL"));
            assert!(stdout.contains("Stress result: FAIL (0/2 rounds completed)"));
            assert!(!stdout.contains("Round 2/2"));
        } else {
            assert!(stdout.is_empty());
        }
        assert!(directory.names().is_empty(), "{args:?}");
    }
}

#[test]
fn timed_cpu_stops_at_the_global_deadline_inside_a_long_phase() {
    let start = std::time::Instant::now();
    let output = successful_output(&[
        "stress",
        "auto",
        "--test",
        "cpu",
        "--duration-secs",
        "1",
        "--duration-ms",
        "5000",
    ]);
    assert!(start.elapsed() < std::time::Duration::from_secs(4));
    assert!(elapsed(&output) >= 0.999);
    assert!(
        field(&output, "CPU iterations: ")
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert_eq!(field(&output, "Stop reason: "), "duration reached");
    assert_eq!(field(&output, "Stress result: "), "PASS");
    assert!(!output.contains("RAM bytes copied:"));
}

#[test]
fn timed_suite_uses_one_storage_budget_and_keeps_running() {
    let directory = ScratchDirectory::create();
    let output = successful_output(&[
        "stress",
        "auto",
        "--duration-secs",
        "1",
        "--duration-ms",
        "2",
        "--buffer-kib",
        "4",
        "--network-mib",
        "1",
        "--storage-dir",
        directory.path(),
        "--storage-mib",
        "1",
    ]);
    assert_eq!(output.matches("Storage: write budget reached").count(), 1);
    assert_eq!(field(&output, "Storage bytes written: "), "1048576");
    assert!(field(&output, "Storage bytes verified: ").starts_with("1048576 "));
    for label in [
        "CPU iterations: ",
        "RAM bytes copied: ",
        "Network bytes received: ",
    ] {
        assert!(!field(&output, label).starts_with("0 "));
    }
    assert_eq!(field(&output, "Stop reason: "), "duration reached");
    assert!(output.lines().count() < 20, "{output}");
    assert!(directory.names().is_empty());
}

#[test]
fn timed_storage_only_finishes_early_after_using_its_budget() {
    let directory = ScratchDirectory::create();
    let output = successful_output(&[
        "stress",
        "auto",
        "--test",
        "storage",
        "--duration-secs",
        "5",
        "--storage-dir",
        directory.path(),
        "--storage-mib",
        "2",
    ]);
    assert_eq!(field(&output, "Storage bytes written: "), "2097152");
    assert_eq!(
        field(&output, "Stop reason: "),
        "storage write budget reached"
    );
    assert!(elapsed(&output) < 5.0);
    assert!(!output.contains("CPU iterations:"));
    assert!(directory.names().is_empty());
}

#[test]
fn timed_stress_rejects_conflicts_and_out_of_range_durations_before_work() {
    let cases: &[(&[&str], &str)] = &[
        (&["--duration-secs", "0"], "integer between 1 and 1209600"),
        (
            &["--duration-secs", "1209601"],
            "integer between 1 and 1209600",
        ),
        (
            &["--duration-secs", "18446744073709551616"],
            "integer between 1 and 1209600",
        ),
        (
            &["--duration-secs", "1209600", "--rounds", "2"],
            "'--rounds' cannot be used",
        ),
        (
            &["--duration-secs", "1", "--timeout-ms", "1000"],
            "'--timeout-ms' cannot be used",
        ),
    ];
    for (options, message) in cases {
        let mut args = vec!["stress", "auto"];
        args.extend_from_slice(options);
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(message),
            "{args:?}"
        );
    }
}

#[test]
fn peripheral_commands_validate_opt_in_and_limits() {
    let help = successful_output(&["--help"]);
    assert!(help.contains("uart --device PATH --loopback"));
    assert!(help.contains("gpio --pin N"));
    let cases: &[(&[&str], &str)] = &[
        (&["uart", "--loopback"], "requires '--device'"),
        (
            &["uart", "--device", "/dev/missing-fixture"],
            "requires '--loopback'",
        ),
        (
            &["uart", "--device", "/dev/null", "--loopback"],
            "recognized serial tty",
        ),
        (
            &["uart", "--device", "Cargo.toml", "--loopback"],
            "serial character device",
        ),
        (&["uart", "--bytes", "65537"], "between 1 and 65536"),
        (&["uart", "--timeout-ms", "0"], "between 1 and 30000"),
        (
            &["uart", "--loopback", "--loopback"],
            "may only be set once",
        ),
        (&["gpio"], "requires '--pin'"),
        (&["gpio", "--pin", "4294967296"], "between 0 and 4294967295"),
        (
            &["gpio", "--pin", "0", "--samples", "100001"],
            "between 1 and 100000",
        ),
        (
            &["gpio", "--pin", "0", "--timeout-ms", "30001"],
            "between 1 and 30000",
        ),
        (&["stress", "auto", "--test", "uart"], "invalid stress test"),
        (&["stress", "auto", "--test", "gpio"], "invalid stress test"),
    ];
    for (args, message) in cases {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

#[test]
fn fixture_bus_commands_reject_unsafe_requests_before_traffic() {
    let help = successful_output(&["--help"]);
    assert!(help.contains("i2c --adapter PATH --address ADDR --fixture"));
    assert!(help.contains("spi --device PATH --loopback"));
    assert!(help.contains("gpio --chip PATH --offset N --input-fixture"));
    let cases: &[(&[&str], &str)] = &[
        (
            &["i2c", "--address", "0x48", "--fixture"],
            "requires '--adapter'",
        ),
        (
            &["i2c", "--adapter", "/dev/null", "--fixture"],
            "requires '--address'",
        ),
        (
            &["i2c", "--adapter", "/dev/null", "--address", "0x48"],
            "requires '--fixture'",
        ),
        (&["i2c", "--address", "0x07"], "unreserved 7-bit address"),
        (&["i2c", "--address", "0x78"], "unreserved 7-bit address"),
        (&["i2c", "--address", "0x100"], "unreserved 7-bit address"),
        (&["i2c", "--read-bytes", "33"], "between 1 and 32"),
        (&["i2c", "--samples", "1025"], "between 1 and 1024"),
        (&["i2c", "--timeout-ms", "0"], "between 1 and 30000"),
        (&["i2c", "--force"], "unknown option"),
        (&["i2c", "--register", "1"], "unknown option"),
        (&["i2c", "--scan"], "unknown option"),
        (
            &[
                "i2c",
                "--adapter",
                "/dev/null",
                "--address",
                "72",
                "--fixture",
            ],
            "expected a i2c-dev device node",
        ),
        (&["spi", "--device", "/dev/null"], "requires '--loopback'"),
        (&["spi", "--loopback"], "requires '--device'"),
        (
            &["spi", "--device", "/dev/null", "--loopback"],
            "expected a spidev device node",
        ),
        (&["spi", "--bytes", "65537"], "between 1 and 65536"),
        (&["spi", "--timeout-ms", "30001"], "between 1 and 30000"),
        (&["spi", "--mode", "0"], "unknown option"),
        (
            &["gpio", "--pin", "23", "--offset", "1"],
            "cannot be combined",
        ),
        (
            &["gpio", "--offset", "1", "--input-fixture"],
            "requires '--chip'",
        ),
        (
            &["gpio", "--chip", "/dev/null", "--input-fixture"],
            "requires '--offset'",
        ),
        (
            &["gpio", "--chip", "/dev/null", "--offset", "0"],
            "requires '--input-fixture'",
        ),
        (
            &["gpio", "--offset", "4294967296"],
            "between 0 and 4294967295",
        ),
        (
            &[
                "gpio",
                "--chip",
                "/dev/null",
                "--offset",
                "0",
                "--input-fixture",
            ],
            "expected a gpio device node",
        ),
        (&["stress", "auto", "--test", "i2c"], "invalid stress test"),
        (&["stress", "auto", "--test", "spi"], "invalid stress test"),
    ];
    for (args, message) in cases {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(stderr.contains(message), "{args:?}: {stderr}");
    }
}

#[test]
fn camera_and_hdmi_require_an_explicit_safe_source() {
    for (args, error) in [
        (&["camera"][..], "requires '--device'"),
        (&["camera", "--frames", "1"], "between 2 and 10000"),
        (&["camera", "--timeout-ms", "30001"], "between 1 and 30000"),
        (&["camera", "--device", "/dev/null"], "video4linux"),
        (
            &["hdmi", "capture", "--device", "/dev/null"],
            "requires '--fixture'",
        ),
        (
            &["hdmi", "status", "--connector", "../../tty"],
            "connector name",
        ),
        (
            &["hdmi", "status", "--device", "/dev/null"],
            "unknown option",
        ),
        (
            &["stress", "auto", "--test", "camera"],
            "invalid stress test",
        ),
    ] {
        let output = run(args);
        assert!(output.stdout.is_empty());
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(
            message.contains(error)
                || (error == "video4linux" && message.contains("supported 64-bit Linux")),
            "{args:?}: {message}"
        );
    }
}

#[test]
fn socketcan_refuses_non_fixture_and_invalid_interface_settings() {
    let common = [
        "--fixture",
        "--interface",
        "lo",
        "--tx-id",
        "0x123",
        "--rx-id",
        "0x124",
        "--session",
        "1234abcd",
    ];
    let cases = [
        (vec!["can", "send"], "requires '--fixture'"),
        (
            vec!["can", "send", "--fixture", "--interface", "../lo"],
            "Linux interface name",
        ),
        (
            vec![
                "can",
                "send",
                "--fixture",
                "--interface",
                "lo",
                "--tx-id",
                "0x800",
            ],
            "standard 11-bit",
        ),
        (
            vec!["can", "receive", "--fixture", "--frames", "4097"],
            "between 1 and 4096",
        ),
        (
            ["can", "send"].into_iter().chain(common).collect(),
            "is not a SocketCAN interface",
        ),
    ];
    for (args, expected) in cases {
        let output = run(&args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(expected),
            "{args:?}"
        );
    }
}

#[test]
fn usb_requires_matching_endpoint_directions_and_a_dedicated_fixture() {
    let complete = [
        "usb",
        "--device",
        "/dev/null",
        "--loopback",
        "--interface",
        "0",
        "--out-endpoint",
        "0x01",
        "--in-endpoint",
        "0x81",
    ];
    for (args, error) in [
        (
            &["usb", "--device", "/dev/null"][..],
            "requires '--loopback'",
        ),
        (
            &["usb", "--loopback", "--device", "/dev/null"],
            "requires '--interface'",
        ),
        (&["usb", "--bytes", "65537"], "between 1 and 65536"),
        (&["usb", "--detach"], "unknown option"),
        (
            &[
                "usb",
                "--loopback",
                "--device",
                "/dev/null",
                "--interface",
                "0",
                "--out-endpoint",
                "0x81",
            ],
            "USB OUT endpoint",
        ),
        (&complete, "expected /dev/bus/usb/BBB/DDD"),
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(error),
            "{args:?}"
        );
    }
}

#[test]
fn pcie_and_iio_reject_unsafe_attributes_and_unbounded_transfer() {
    for (args, error) in [
        (&["pcie", "link"][..], "requires '--device'"),
        (
            &["pcie", "link", "--device", "0000:00:20.0"],
            "complete PCI device ID",
        ),
        (
            &["pcie", "throughput", "--device", "0000:01:00.0"],
            "requires '--fixture'",
        ),
        (
            &[
                "pcie",
                "throughput",
                "--device",
                "0000:01:00.0",
                "--fixture",
            ],
            "no bounded completion timeout",
        ),
        (&["iio", "--device", "iio:device0"], "requires '--channel'"),
        (
            &[
                "iio",
                "--device",
                "iio:device0",
                "--channel",
                "../scan_elements",
            ],
            "input channel stem",
        ),
        (
            &["iio", "--device", "iio:device0", "--channel", "in_temp_raw"],
            "without the '_raw' suffix",
        ),
        (
            &["iio", "--device", "../iio:device0", "--channel", "in_temp"],
            "explicit IIO device name",
        ),
        (&["iio", "--samples", "1"], "between 2 and 10000"),
        (&["stress", "auto", "--test", "iio"], "invalid stress test"),
    ] {
        let output = run(args);
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(error),
            "{args:?}"
        );
    }
}

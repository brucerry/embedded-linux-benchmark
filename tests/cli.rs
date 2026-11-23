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

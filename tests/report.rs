use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::Command;

struct ReportFiles {
    directory: PathBuf,
    markdown: PathBuf,
    pdf: PathBuf,
}

impl Drop for ReportFiles {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.markdown);
        let _ = fs::remove_file(&self.pdf);
        let _ = fs::remove_dir(&self.directory);
    }
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_embedded-linux-benchmark"))
}

fn run_report(args: &[&str], expected_slug: &str) -> ReportFiles {
    let output = command().args(args).output().unwrap();
    assert!(
        output.status.success() && output.stderr.is_empty(),
        "{output:?}"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let find = |prefix| {
        PathBuf::from(
            stdout
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap(),
        )
    };
    let markdown = find("Report: ");
    let pdf = find("Plots: ");
    let directory = markdown.parent().unwrap().to_path_buf();
    assert_eq!(directory.parent().unwrap().to_str(), Some("/tmp"));
    assert!(directory
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("embedded-linux-benchmark-"));
    assert_eq!(pdf.parent().unwrap(), directory);
    assert_eq!(markdown.extension().unwrap(), "md");
    assert_eq!(pdf.extension().unwrap(), "pdf");
    assert_eq!(markdown.file_stem(), pdf.file_stem());
    let contents = fs::read_to_string(&markdown).unwrap();
    let start = contents
        .lines()
        .find_map(|line| line.strip_prefix("| Start (UTC) | "))
        .and_then(|line| line.strip_suffix(" |"))
        .unwrap();
    assert!(start.contains('T') && start.ends_with('Z'));
    let stamp = start.replace(':', "-");
    assert_eq!(
        markdown.file_stem().unwrap(),
        format!("{expected_slug}_{stamp}").as_str()
    );
    assert!(contents.contains(&format!(
        "[PDF report]({})",
        pdf.file_name().unwrap().to_str().unwrap()
    )));
    let pdf_contents = fs::read(&pdf).unwrap();
    assert!(String::from_utf8_lossy(&pdf_contents).contains(&format!(r"Start \(UTC\): {start}")));
    ReportFiles {
        directory,
        markdown,
        pdf,
    }
}

#[test]
fn cpu_command_creates_private_markdown_and_vector_pdf() {
    let first = run_report(&["cpu", "--duration-ms", "1"], "cpu");
    let second = run_report(&["cpu", "--duration-ms", "1"], "cpu");
    assert_ne!(first.directory, second.directory);
    assert_eq!(
        fs::metadata(&first.directory).unwrap().mode() & 0o777,
        0o700
    );
    for path in [&first.markdown, &first.pdf] {
        let metadata = fs::symlink_metadata(path).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.mode() & 0o777, 0o600);
        assert_eq!(metadata.nlink(), 1);
    }
    let markdown = fs::read_to_string(&first.markdown).unwrap();
    assert!(markdown.starts_with("# Embedded Linux Benchmark report: CPU\n"));
    assert!(markdown.contains("| Test item | CPU |"));
    assert!(markdown.contains("| Status | Completed |"));
    assert!(markdown.contains("| CPU throughput |"));
    assert!(markdown.contains("| iterations/s |"));
    assert!(markdown.contains("    Checksum:"));
    let pdf = fs::read(&first.pdf).unwrap();
    assert!(pdf.starts_with(b"%PDF-1.4\n"));
    assert!(pdf.windows(5).any(|window| window == b" re f"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("CPU throughput"));
    assert!(text.contains("Measured values \\(iterations/s\\)"));
}

#[test]
fn storage_plots_independent_measured_read_and_write_values() {
    let report = run_report(&["storage", "--dir", "/tmp", "--size-mib", "1"], "storage");
    let markdown = fs::read_to_string(&report.markdown).unwrap();
    assert!(markdown.contains("Sequential write throughput"));
    assert!(markdown.contains("Sequential read throughput"));
    assert!(markdown.contains("page cache eligible"));
    let pdf = fs::read(&report.pdf).unwrap();
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/Count 1"));
    assert!(text.contains("Measured values \\(MiB/s\\)"));
    assert!(text.contains("Sequential write throughput"));
    assert!(text.contains("Sequential read throughput"));
}

#[test]
fn help_and_validation_errors_have_no_report_output() {
    let output = command().arg("--help").output().unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8(output.stdout)
        .unwrap()
        .contains("Report: "));
    let output = command()
        .args(["cpu", "--duration-ms", "0"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn stress_reports_aggregate_measurements() {
    let report = run_report(
        &[
            "stress",
            "auto",
            "--test",
            "cpu",
            "--rounds",
            "2",
            "--duration-ms",
            "1",
        ],
        "stress-cpu",
    );
    let markdown = fs::read_to_string(&report.markdown).unwrap();
    assert!(markdown.starts_with("# Embedded Linux Benchmark report: Stress: CPU\n"));
    assert!(markdown.contains("| Selected workloads | CPU |"));
    assert!(!markdown.contains("| Passive observations |"));
    assert!(markdown.contains("Stress result: PASS (2/2 rounds completed)"));
    assert!(markdown.contains("| Stress CPU throughput |"));
    assert!(markdown.contains("CPU iterations:"));
    assert!(!markdown.contains("Round 1/2"));
    assert!(!markdown.contains("Checksum:"));
    let pdf = fs::read(&report.pdf).unwrap();
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(pdf.contains("Stress: CPU"));
    assert!(pdf.contains("Stress CPU throughput"));
    assert!(!pdf.contains("Stress RAM copy throughput"));
}

#[test]
fn mixed_stress_identifies_selected_workloads_and_separates_passive_observations() {
    let base_args = [
        "stress",
        "auto",
        "--rounds",
        "1",
        "--duration-ms",
        "1",
        "--buffer-kib",
        "4",
        "--network-mib",
        "1",
    ];
    for include_storage in [false, true] {
        let mut args = base_args.to_vec();
        let (slug, items) = if include_storage {
            args.extend(["--storage-dir", "/tmp", "--storage-mib", "1"]);
            (
                "stress-cpu-ram-tcp-loopback-storage",
                "CPU, RAM, TCP loopback, storage",
            )
        } else {
            ("stress-cpu-ram-tcp-loopback", "CPU, RAM, TCP loopback")
        };
        let report = run_report(&args, slug);
        let markdown = fs::read_to_string(&report.markdown).unwrap();
        assert!(markdown.contains(&format!("| Selected workloads | {items} |")));
        assert!(markdown.contains("| Passive observations | PCIe, USB, HDMI (read-only"));
        assert!(!markdown.contains("| HDMI status observation |"));
        let bytes = fs::read(&report.pdf).unwrap();
        let pdf = String::from_utf8_lossy(&bytes);
        assert!(pdf.contains(&format!("Stress: {items}")));
        assert!(pdf.contains("Passive observations: PCIe, USB, HDMI"));
        assert_eq!(
            markdown.contains("| Stress storage verified round-trip throughput |"),
            include_storage
        );
    }
}

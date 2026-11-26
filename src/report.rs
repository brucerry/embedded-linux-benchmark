use std::cell::RefCell;
use std::fmt::{self, Write as _};
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{self, Write as _};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REPORT_ROOT: &str = "/tmp";
const TRANSCRIPT_LIMIT: usize = 8192;
const METRIC_LIMIT: usize = 32;
const CHART_ROWS: usize = 10;
const TEXT_ROWS: usize = 28;

thread_local! {
    static CAPTURE: RefCell<Option<Report>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy)]
struct Metric {
    label: &'static str,
    value: f64,
    unit: &'static str,
}

// Subject names are fixed program values, never user arguments or device output.
#[derive(Clone, Copy)]
pub(crate) enum Subject {
    Cpu,
    Ram,
    Storage,
    NetworkLoopback,
    NetworkPeerServer,
    NetworkPeerClient,
    Uart,
    Usb,
    CanSend,
    CanReceive,
    PcieLink,
    PcieThroughput,
    Iio,
    Camera,
    HdmiStatus,
    HdmiCapture,
    I2c,
    Spi,
    GpioSysfs,
    GpioCharacterDevice,
    Stress {
        test: Option<crate::stress::Test>,
        storage: bool,
    },
}

impl Subject {
    fn names(self) -> (&'static str, &'static str) {
        use crate::stress::Test;
        match self {
            Self::Cpu => ("cpu", "CPU"),
            Self::Ram => ("ram", "RAM"),
            Self::Storage => ("storage", "Storage"),
            Self::NetworkLoopback => ("tcp-loopback", "TCP loopback"),
            Self::NetworkPeerServer => ("tcp-peer-server", "TCP peer server"),
            Self::NetworkPeerClient => ("tcp-peer-client", "TCP peer client"),
            Self::Uart => ("uart-loopback", "UART loopback"),
            Self::Usb => ("usb-loopback", "USB loopback"),
            Self::CanSend => ("can-send", "CAN send"),
            Self::CanReceive => ("can-receive", "CAN receive"),
            Self::PcieLink => ("pcie-link", "PCIe link observation"),
            Self::PcieThroughput => ("pcie-throughput", "PCIe throughput"),
            Self::Iio => ("iio", "IIO sampling"),
            Self::Camera => ("camera", "Camera capture"),
            Self::HdmiStatus => ("hdmi-status", "HDMI status observation"),
            Self::HdmiCapture => ("hdmi-capture", "HDMI capture"),
            Self::I2c => ("i2c-read", "I2C read"),
            Self::Spi => ("spi-loopback", "SPI loopback"),
            Self::GpioSysfs => ("gpio-sysfs", "GPIO sysfs input"),
            Self::GpioCharacterDevice => ("gpio-chardev", "GPIO character-device input"),
            Self::Stress {
                test: Some(Test::Cpu),
                ..
            } => ("stress-cpu", "CPU"),
            Self::Stress {
                test: Some(Test::Ram),
                ..
            } => ("stress-ram", "RAM"),
            Self::Stress {
                test: Some(Test::Network),
                ..
            } => ("stress-tcp-loopback", "TCP loopback"),
            Self::Stress {
                test: Some(Test::Storage),
                ..
            } => ("stress-storage", "Storage"),
            Self::Stress {
                test: None,
                storage: false,
            } => ("stress-cpu-ram-tcp-loopback", "CPU, RAM, TCP loopback"),
            Self::Stress {
                test: None,
                storage: true,
            } => (
                "stress-cpu-ram-tcp-loopback-storage",
                "CPU, RAM, TCP loopback, storage",
            ),
        }
    }

    fn is_stress(self) -> bool {
        matches!(self, Self::Stress { .. })
    }

    fn observes_hardware(self) -> bool {
        matches!(self, Self::Stress { test: None, .. })
    }
}

struct Identity {
    subject: Subject,
    utc: String,
    stem: String,
}

impl Identity {
    fn new(subject: Subject, started_at: Option<Duration>) -> Self {
        let utc = started_at.map_or_else(|| "unavailable".into(), utc);
        let stem = format!("{}_{}", subject.names().0, utc.replace(':', "-"));
        Self { subject, utc, stem }
    }

    fn title(&self) -> String {
        let items = self.subject.names().1;
        if self.subject.is_stress() {
            format!("Stress: {items}")
        } else {
            items.into()
        }
    }

    fn item_label(&self) -> &'static str {
        if self.subject.is_stress() {
            "Selected workloads"
        } else {
            "Test item"
        }
    }
}

struct Report {
    start: Instant,
    started_at: Option<Duration>,
    subject: Option<Subject>,
    output: String,
    truncated: bool,
    metrics: Vec<Metric>,
}

pub(crate) fn begin() {
    CAPTURE.with(|state| {
        *state.borrow_mut() = Some(Report {
            start: Instant::now(),
            started_at: SystemTime::now().duration_since(UNIX_EPOCH).ok(),
            subject: None,
            output: String::new(),
            truncated: false,
            metrics: Vec::new(),
        });
    });
}

pub(crate) fn identify(subject: Subject) {
    CAPTURE.with(|state| {
        if let Some(report) = state.borrow_mut().as_mut() {
            report.subject = Some(subject);
        }
    });
}

// Shared benchmark descriptions stream to the console only inside stress rounds.
pub(crate) fn emit(record: bool, args: fmt::Arguments<'_>) {
    if record {
        line(args);
    } else {
        println!("{args}");
    }
}

pub(crate) fn line(args: fmt::Arguments<'_>) {
    let line = args.to_string();
    println!("{line}");
    CAPTURE.with(|state| {
        if let Some(report) = state.borrow_mut().as_mut() {
            report.append(&line);
        }
    });
}

// Measurements are explicit rather than parsed from console output or untrusted device names.
pub(crate) fn metric(label: &'static str, value: f64, unit: &'static str) {
    if value.is_finite() && value >= 0.0 {
        CAPTURE.with(|state| {
            if let Some(report) = state.borrow_mut().as_mut() {
                if report.metrics.len() < METRIC_LIMIT {
                    report.metrics.push(Metric { label, value, unit });
                }
            }
        });
    }
}

pub(crate) struct Files {
    pub(crate) markdown: PathBuf,
    pub(crate) pdf: PathBuf,
}

pub(crate) fn finish(result: &Result<(), String>) -> io::Result<Option<Files>> {
    let report = CAPTURE.with(|state| state.borrow_mut().take());
    let Some(mut report) = report.filter(|report| !report.output.is_empty()) else {
        return Ok(None);
    };
    let success = result.is_ok();
    let elapsed = report.start.elapsed().as_secs_f64();
    if let Err(error) = result {
        report.append("Error:");
        report.append(error);
    }
    let status = if success { "Completed" } else { "Failed" };
    let subject = report
        .subject
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing benchmark identity"))?;
    report
        .write(
            Path::new(REPORT_ROOT),
            &Identity::new(subject, report.started_at),
            status,
            elapsed,
        )
        .map(Some)
}

impl Report {
    fn append(&mut self, text: &str) {
        if self.truncated {
            return;
        }
        // Replace controls (e.g. terminal escapes) without breaking UTF-8 or markdown structure.
        for ch in text.chars().chain(std::iter::once('\n')) {
            let ch = if ch != '\n' && ch.is_control() {
                '?'
            } else {
                ch
            };
            if self.output.len() + ch.len_utf8() > TRANSCRIPT_LIMIT {
                self.truncated = true;
                return;
            }
            self.output.push(ch);
        }
    }

    fn write(
        &self,
        root: &Path,
        identity: &Identity,
        status: &str,
        elapsed: f64,
    ) -> io::Result<Files> {
        let directory = private_directory(root)?;
        let pdf_name = format!("{}.pdf", identity.stem);
        let markdown = directory.join(format!("{}.md", identity.stem));
        let pdf = directory.join(&pdf_name);
        let result = (|| {
            write_new(
                &markdown,
                self.markdown(identity, &pdf_name, status, elapsed)
                    .as_bytes(),
            )?;
            write_new(&pdf, &self.pdf(identity, status, elapsed))
        })();
        if result.is_err() {
            // Only remove names within this freshly created, owner-only directory.
            let _ = fs::remove_file(&markdown);
            let _ = fs::remove_file(&pdf);
            let _ = fs::remove_dir(&directory);
        }
        result.map(|_| Files { markdown, pdf })
    }

    fn markdown(&self, identity: &Identity, pdf_name: &str, status: &str, elapsed: f64) -> String {
        let mut out = format!(
            "# Embedded Linux Diagnostic Engine report: {}\n\n| Run | Value |\n| --- | --- |\n| Status | {status} |\n| Wall time | {elapsed:.3} s |\n",
            identity.title()
        );
        let _ = writeln!(
            out,
            "| {} | {} |\n| Start (UTC) | {} |",
            identity.item_label(),
            identity.subject.names().1,
            identity.utc
        );
        if identity.subject.observes_hardware() {
            out.push_str(
                "| Passive observations | PCIe, USB, HDMI (read-only, when available) |\n",
            );
        }
        let _ = writeln!(
            out,
            "| Platform | {} / {} |",
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        out.push_str("\n## Measurements\n\n");
        if self.metrics.is_empty() {
            out.push_str(
                "No quantitative measurements were recorded. See the observations below.\n",
            );
        } else {
            out.push_str("| Measurement | Value | Unit |\n| --- | ---: | --- |\n");
            for m in &self.metrics {
                let _ = writeln!(out, "| {} | {} | {} |", m.label, number(m.value), m.unit);
            }
        }
        let _ = writeln!(
            out,
            "\n[PDF report]({pdf_name}) shows measurements grouped by unit, or observations when there are no measurements."
        );
        out.push_str("\n## Benchmark output\n\n");
        for line in self.output.lines() {
            let _ = writeln!(out, "    {line}");
        }
        if self.truncated {
            out.push_str("\n_Output truncated at the report size limit._\n");
        }
        out
    }

    fn pdf(&self, identity: &Identity, status: &str, elapsed: f64) -> Vec<u8> {
        let mut groups: Vec<(&str, Vec<Metric>)> = Vec::new();
        for &metric in &self.metrics {
            if let Some((_, entries)) = groups.iter_mut().find(|(unit, _)| *unit == metric.unit) {
                entries.push(metric);
            } else {
                groups.push((metric.unit, vec![metric]));
            }
        }
        let mut pages = Vec::new();
        if groups.is_empty() {
            let mut page = page_title(identity, status, elapsed);
            pdf_text(
                &mut page,
                45,
                631,
                13,
                "Observations (no quantitative plot)",
            );
            for (i, line) in self.output.lines().take(TEXT_ROWS).enumerate() {
                pdf_text(&mut page, 45, 606 - i * 17, 10, line);
            }
            pdf_text(
                &mut page,
                45,
                65,
                10,
                "See the paired Markdown report for the benchmark output.",
            );
            pages.push(page);
        } else {
            for (unit, metrics) in groups {
                let maximum = metrics.iter().map(|m| m.value).fold(0.0_f64, f64::max);
                for rows in metrics.chunks(CHART_ROWS) {
                    let mut page = page_title(identity, status, elapsed);
                    draw_chart(&mut page, unit, maximum, rows);
                    pages.push(page);
                }
            }
        }
        pdf_document(&pages)
    }
}

fn private_directory(root: &Path) -> io::Result<PathBuf> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..32 {
        let dir = root.join(format!(
            "eld-engine-{}-{timestamp}-{attempt}",
            std::process::id()
        ));
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "report directory collision",
    ))
}

fn write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(contents)
}

fn utc(timestamp: Duration) -> String {
    let seconds = timestamp.as_secs();
    // Shift from the Unix epoch to a March-based Gregorian calendar so leap day is last.
    let days = seconds / 86_400 + 719_468;
    let cycle = days / 146_097;
    let day_in_cycle = days % 146_097;
    let year_in_cycle =
        (day_in_cycle - day_in_cycle / 1460 + day_in_cycle / 36_524 - day_in_cycle / 146_096) / 365;
    let day_in_year =
        day_in_cycle - (365 * year_in_cycle + year_in_cycle / 4 - year_in_cycle / 100);
    let month_from_march = (5 * day_in_year + 2) / 153;
    let day = day_in_year - (153 * month_from_march + 2) / 5 + 1;
    let month = (month_from_march + 2) % 12 + 1;
    let year = cycle * 400 + year_in_cycle + u64::from(month <= 2);
    let hour = seconds / 3600 % 24;
    let minute = seconds / 60 % 60;
    let second = seconds % 60;
    let nanos = timestamp.subsec_nanos();
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{nanos:09}Z")
}

fn number(value: f64) -> String {
    if value >= 1_000_000.0 || (value > 0.0 && value < 0.001) {
        format!("{value:.4e}")
    } else {
        format!("{value:.3}")
    }
}

fn page_title(identity: &Identity, status: &str, elapsed: f64) -> String {
    let mut page = String::new();
    pdf_text(&mut page, 45, 745, 20, "Embedded Linux Benchmark");
    pdf_text(&mut page, 45, 718, 13, &identity.title());
    pdf_text(
        &mut page,
        45,
        698,
        11,
        &format!("Start (UTC): {}", identity.utc),
    );
    pdf_text(
        &mut page,
        45,
        678,
        11,
        &format!("{status}  |  Wall time {elapsed:.3} s"),
    );
    if identity.subject.observes_hardware() {
        pdf_text(
            &mut page,
            45,
            657,
            10,
            "Passive observations: PCIe, USB, HDMI (read-only, when available)",
        );
    }
    page
}

fn draw_chart(page: &mut String, unit: &str, maximum: f64, metrics: &[Metric]) {
    pdf_text(page, 45, 631, 13, &format!("Measured values ({unit})"));
    for (i, metric) in metrics.iter().enumerate() {
        let y = 600 - i * 48;
        pdf_text(page, 45, y + 16, 10, metric.label);
        pdf_text(page, 443, y + 16, 10, &number(metric.value));
        let width = if maximum > 0.0 {
            505.0 * (metric.value / maximum)
        } else {
            0.0
        };
        let _ = writeln!(page, "0.88 0.91 0.95 rg 45 {y} 505 8 re f");
        let _ = writeln!(page, "0.07 0.30 0.55 rg 45 {y} {width:.2} 8 re f");
    }
    let axis = 575 - (metrics.len() - 1) * 48;
    pdf_text(page, 45, axis, 10, "0");
    pdf_text(page, 443, axis, 10, &number(maximum));
    pdf_text(
        page,
        45,
        axis - 22,
        10,
        "Linear scale from zero to the largest value in this unit.",
    );
}

fn pdf_text(page: &mut String, x: usize, y: usize, size: usize, value: &str) {
    // PDF standard Helvetica with printable ASCII only. Escape string operators.
    let _ = write!(page, "0.10 0.14 0.20 rg BT /F1 {size} Tf {x} {y} Td (");
    for ch in value.chars().take(84) {
        match ch {
            '(' | ')' | '\\' => {
                page.push('\\');
                page.push(ch);
            }
            ' '..='~' => page.push(ch),
            _ => page.push('?'),
        }
    }
    page.push_str(") Tj ET\n");
}

fn pdf_document(pages: &[String]) -> Vec<u8> {
    // One Catalog, one Pages node and one standard font; each page has one uncompressed stream.
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = vec![0_usize];
    let mut object = |body: &str| {
        let id = offsets.len();
        offsets.push(out.len());
        let _ = writeln!(out, "{id} 0 obj\n{body}\nendobj");
    };
    object("<< /Type /Catalog /Pages 2 0 R >>");
    let children: String = (0..pages.len())
        .map(|i| format!("{} 0 R ", 4 + 2 * i))
        .collect();
    object(&format!(
        "<< /Type /Pages /Count {} /Kids [ {children}] >>",
        pages.len()
    ));
    object("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>");
    for (i, page) in pages.iter().enumerate() {
        object(&format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        object(&format!(
            "<< /Length {} >>\nstream\n{page}endstream",
            page.len()
        ));
    }
    let start = out.len();
    let _ = writeln!(out, "xref\n0 {}\n0000000000 65535 f ", offsets.len());
    for offset in &offsets[1..] {
        let _ = writeln!(out, "{offset:010} 00000 n ");
    }
    let _ = writeln!(
        out,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF",
        offsets.len()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_use_utc_including_leap_days() {
        assert_eq!(utc(Duration::ZERO), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(
            utc(Duration::new(951_827_696, 123)),
            "2000-02-29T12:34:56.000000123Z"
        );
        assert_eq!(
            utc(Duration::from_secs(4_107_542_400)),
            "2100-03-01T00:00:00.000000000Z"
        );
    }

    #[test]
    fn filename_identity_is_safe_and_does_not_invent_a_timestamp() {
        let at = Some(Duration::new(951_827_696, 123));
        let identity = Identity::new(Subject::HdmiStatus, at);
        assert_eq!(identity.stem, "hdmi-status_2000-02-29T12-34-56.000000123Z");
        assert_eq!(identity.title(), "HDMI status observation");
        let identity = Identity::new(Subject::Uart, None);
        assert_eq!(identity.stem, "uart-loopback_unavailable");
        assert_eq!(identity.utc, "unavailable");
    }

    #[test]
    fn transcript_stays_utf8_and_bounded() {
        begin();
        CAPTURE.with(|state| {
            let mut state = state.borrow_mut();
            let report = state.as_mut().unwrap();
            report.append(&"a".repeat(TRANSCRIPT_LIMIT - 1));
            assert_eq!(report.output.len(), TRANSCRIPT_LIMIT);
            report.append("語");
            assert!(report.truncated);
            assert_eq!(report.output.len(), TRANSCRIPT_LIMIT);
        });
    }

    #[test]
    fn graph_accepts_only_finite_results_and_groups_units() {
        begin();
        metric("Throughput", 20.0, "MiB/s");
        metric("Latency", 0.0, "ms");
        metric("Invalid", f64::NAN, "ms");
        metric("Invalid", f64::INFINITY, "ms");
        CAPTURE.with(|state| {
            let mut state = state.borrow_mut();
            let report = state.as_mut().unwrap();
            report.append("CPU (fixture) \\ value");
            assert_eq!(report.metrics.len(), 2);
            let identity = Identity::new(Subject::Cpu, Some(Duration::ZERO));
            let pdf = report.pdf(&identity, "Completed", 0.1);
            let text = String::from_utf8_lossy(&pdf);
            assert!(text.contains("/Count 2"));
            assert!(!text.contains("fixture"));
            assert!(text.contains(r"Start \(UTC\): 1970-01-01T00:00:00.000000000Z"));
            assert!(text.contains("Measured values \\(MiB/s\\)"));
            assert!(!text.contains("NaN"));
        });
    }

    #[test]
    fn observation_report_has_identity_without_inventing_a_plot() {
        begin();
        CAPTURE.with(|state| {
            let mut state = state.borrow_mut();
            let report = state.as_mut().unwrap();
            report.append("HDMI (fixture) \\ value");
            let identity = Identity::new(Subject::HdmiStatus, Some(Duration::ZERO));
            let markdown = report.markdown(&identity, "test.pdf", "Completed", 0.1);
            assert!(markdown.contains("No quantitative measurements were recorded."));
            assert!(markdown.contains("[PDF report](test.pdf)"));
            let bytes = report.pdf(&identity, "Completed", 0.1);
            let pdf = String::from_utf8_lossy(&bytes);
            assert!(pdf.contains("HDMI status observation"));
            assert!(pdf.contains(r"HDMI \(fixture\) \\ value"));
            assert!(!pdf.contains("Measured values"));
        });
    }
}

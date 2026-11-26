use crate::common::{Deadline, RunError, Sample, MIB};
use crate::{cpu, network, ram, storage};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[path = "stress_probe.rs"]
mod probes;

pub(crate) const DEFAULT_ROUNDS: u64 = 3;
pub(crate) const MAX_ROUNDS: u64 = 5;
pub(crate) const DEFAULT_DURATION_MS: u64 = 10_000;
pub(crate) const MAX_DURATION_MS: u64 = 60_000;
pub(crate) const DEFAULT_BUFFER_KIB: u64 = 1_024;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 60_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 360_000;
pub(crate) const MAX_DURATION_SECS: u64 = 14 * 24 * 60 * 60;

pub(crate) fn parse_duration(value: &str) -> Result<u64, String> {
    let (number, scale) = match value.as_bytes().last() {
        Some(b's') => (&value[..value.len() - 1], 1),
        Some(b'm') => (&value[..value.len() - 1], 60),
        Some(b'h') => (&value[..value.len() - 1], 60 * 60),
        Some(b'd') => (&value[..value.len() - 1], 24 * 60 * 60),
        Some(b'w') => (&value[..value.len() - 1], 7 * 24 * 60 * 60),
        _ => (value, 1),
    };
    if !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()) {
        if let Some(seconds) = number
            .parse::<u64>()
            .ok()
            .and_then(|n| n.checked_mul(scale))
        {
            if (1..=MAX_DURATION_SECS).contains(&seconds) {
                return Ok(seconds);
            }
        }
    }
    Err(
        "'--duration' must be a whole duration from 1s through 14d (for example 30m, 12h, 2w)"
            .into(),
    )
}
const PROGRESS_INTERVAL: Duration = Duration::from_secs(300);
const STORAGE_CHUNK_BYTES: u64 = MIB;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Test {
    Cpu,
    Ram,
    Network,
    Storage,
}

impl Test {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Ram => "ram",
            Self::Network => "network",
            Self::Storage => "storage",
        }
    }
}

pub(crate) struct Storage {
    pub(crate) directory: PathBuf,
    pub(crate) total_bytes: u64,
    pub(crate) sync: bool,
}

pub(crate) struct Plan {
    pub(crate) test: Option<Test>,
    pub(crate) schedule: Schedule,
    pub(crate) duration: Duration,
    pub(crate) buffer_kib: u64,
    pub(crate) network_bytes: u64,
    pub(crate) storage: Option<Storage>,
    pub(crate) summary_path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
pub(crate) enum Schedule {
    Rounds { count: u64, timeout: Duration },
    Timed(Duration),
}

impl Plan {
    fn includes(&self, test: Test) -> bool {
        self.test.map_or(true, |selected| selected == test)
    }
}

#[derive(Default)]
struct Totals {
    cpu: Sample,
    ram: Sample,
    network: Sample,
    storage_written: u64,
    storage_verified: Sample,
}

struct Worker<T> {
    name: &'static str,
    commands: Option<SyncSender<Deadline>>,
    results: Receiver<Result<(T, bool), RunError>>,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Worker<T> {
    fn new(
        name: &'static str,
        run: impl Fn(Deadline) -> Result<(T, bool), RunError> + Send + 'static,
    ) -> Result<Self, String> {
        let (send_command, commands) = sync_channel(1);
        let (send_result, results) = sync_channel(1);
        let thread = thread::Builder::new()
            .name(format!("benchmark-{name}"))
            .spawn(move || {
                while let Ok(deadline) = commands.recv() {
                    if send_result.send(run(deadline)).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| format!("{name}: failed to start stress worker: {error}"))?;
        Ok(Self {
            name,
            commands: Some(send_command),
            results,
            thread: Some(thread),
        })
    }

    fn start(&self, deadline: Deadline) -> Result<(), String> {
        self.commands
            .as_ref()
            .expect("worker still active")
            .send(deadline)
            .map_err(|_| format!("{}: stress worker exited early", self.name))
    }

    fn finish(&self, deadline: Deadline, print: bool) -> Result<Option<(T, bool)>, String> {
        let result = self
            .results
            .recv()
            .map_err(|_| format!("{}: stress worker exited early", self.name))?;
        match result {
            Ok(sample) => Ok(Some(sample)),
            Err(RunError::Deadline) if deadline.check().is_err() => Ok(None),
            Err(error) => {
                if print {
                    println!("{}: FAIL", self.name);
                }
                Err(format!("{}: {error}", self.name))
            }
        }
    }
}

impl<T> Drop for Worker<T> {
    fn drop(&mut self) {
        drop(self.commands.take());
        // There is at most one pending result per worker and a queue slot for it.
        // A failed sibling therefore cannot leave a worker blocked trying to report.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

type CpuWorker = Worker<(Sample, u32)>;

struct Workers {
    cpu: Option<CpuWorker>,
    ram: Option<Worker<Sample>>,
    network: Option<Worker<Sample>>,
}

impl Workers {
    fn new(plan: &Plan) -> Result<Self, String> {
        let duration = plan.duration;
        let buffer_kib = plan.buffer_kib;
        let network_bytes = plan.network_bytes;
        Ok(Self {
            cpu: if plan.includes(Test::Cpu) {
                Some(Worker::new("CPU", move |deadline| {
                    let result = cpu::benchmark(duration, Some(deadline))?;
                    let full = result.0.elapsed >= duration && deadline.check().is_ok();
                    Ok((result, full))
                })?)
            } else {
                None
            },
            ram: if plan.includes(Test::Ram) {
                Some(Worker::new("RAM", move |deadline| {
                    let result = ram::benchmark(duration, buffer_kib, Some(deadline))?;
                    let full = result.elapsed >= duration && deadline.check().is_ok();
                    Ok((result, full))
                })?)
            } else {
                None
            },
            network: if plan.includes(Test::Network) {
                Some(Worker::new("Network", move |deadline| {
                    network::sustained(network_bytes, duration, deadline)
                })?)
            } else {
                None
            },
        })
    }

    fn start(&self, deadline: Deadline) -> Result<(), String> {
        if let Some(worker) = &self.cpu {
            worker.start(deadline)?;
        }
        if let Some(worker) = &self.ram {
            worker.start(deadline)?;
        }
        if let Some(worker) = &self.network {
            worker.start(deadline)?;
        }
        Ok(())
    }

    fn finish(
        &self,
        plan: &Plan,
        totals: &mut Totals,
        deadline: Deadline,
        print: bool,
    ) -> Result<bool, String> {
        let mut full = true;
        if let Some(worker) = &self.cpu {
            match worker.finish(deadline, print)? {
                Some(((sample, state), done)) => {
                    if print && done {
                        cpu::describe(&sample, state, false);
                        println!("CPU: PASS");
                    }
                    totals.cpu.add(sample);
                    full &= done;
                }
                None => full = false,
            }
        }
        if let Some(worker) = &self.ram {
            match worker.finish(deadline, print)? {
                Some((sample, done)) => {
                    if print && done {
                        ram::describe(&sample, plan.buffer_kib, false);
                        println!("RAM: PASS");
                    }
                    totals.ram.add(sample);
                    full &= done;
                }
                None => full = false,
            }
        }
        if let Some(worker) = &self.network {
            match worker.finish(deadline, print)? {
                Some((sample, done)) => {
                    if print && done {
                        network::describe(&sample, false);
                        println!("Network: PASS");
                    }
                    totals.network.add(sample);
                    full &= done;
                }
                None => full = false,
            }
        }
        Ok(full)
    }
}

fn service_storage(
    plan: &Plan,
    totals: &mut Totals,
    deadline: Deadline,
    fixed_round: Option<(u64, u64)>,
) -> Result<bool, String> {
    let storage = match &plan.storage {
        Some(storage) => storage,
        None => return Ok(true),
    };
    let remaining = storage.total_bytes - totals.storage_written;
    if remaining == 0 {
        return Ok(true); // Explicit wear budget exhausted, not an additional storage pass.
    }
    let bytes = match fixed_round {
        Some((round, rounds)) if round == rounds => remaining,
        Some((_, rounds)) => storage.total_bytes / rounds,
        None => remaining.min(STORAGE_CHUNK_BYTES),
    };
    let bounded = deadline.capped(Duration::from_millis(storage::DEFAULT_TIMEOUT_MS));
    let result = storage::benchmark(
        &storage.directory,
        bytes,
        storage.sync,
        bounded,
        &mut totals.storage_written,
    );
    if totals.storage_written == storage.total_bytes {
        println!(
            "Storage: write budget reached ({} bytes)",
            storage.total_bytes
        );
    }
    match result {
        Ok((write, read)) => {
            if fixed_round.is_some() {
                storage::describe(&storage.directory, bytes, storage.sync, write, read, false);
                println!("Storage: PASS");
            }
            totals.storage_verified.add(Sample {
                count: bytes,
                elapsed: write + read,
            });
            Ok(true)
        }
        Err(RunError::Deadline) if bounded == deadline => Ok(false),
        Err(error) => {
            println!("Storage: FAIL");
            Err(format!("Storage: {error}"))
        }
    }
}

fn run_round(
    plan: &Plan,
    workers: &Workers,
    probes: &mut Option<probes::Probes>,
    totals: &mut Totals,
    deadline: Deadline,
    fixed_round: Option<(u64, u64)>,
) -> Result<bool, String> {
    workers.start(deadline)?;
    // Keep CPU/RAM/loopback busy while storage and passive observations run.
    let storage = service_storage(plan, totals, deadline, fixed_round);
    let observed = probes
        .as_mut()
        .map_or(true, |probe| probe.observe(deadline));
    let core = workers.finish(plan, totals, deadline, fixed_round.is_some())?;
    let storage = storage?;
    Ok(core && storage && observed)
}

fn describe_tests(plan: &Plan) {
    if let Some(test) = plan.test {
        report_line!("Test: {}", test.name());
    } else {
        report_line!(
            "Automatic load: CPU, RAM, TCP loopback; read-only PCIe, USB and HDMI observations"
        );
        report_line!("Fixture-only device, peer and accelerator tests: skipped (run separately)");
    }
    if plan.test != Some(Test::Storage) {
        report_line!("Target per load phase: {} ms", plan.duration.as_millis());
    }
    match (&plan.storage, plan.test) {
        (Some(storage), _) => report_line!(
            "Storage write budget: {} bytes across all rounds",
            storage.total_bytes
        ),
        (None, None) => {
            report_line!("Storage: disabled (explicit directory and write budget required)")
        }
        (None, Some(_)) => (),
    }
}

fn describe_totals(plan: &Plan, totals: &Totals) {
    for (test, name, sample) in [
        (Test::Cpu, "CPU iterations", &totals.cpu),
        (Test::Ram, "RAM bytes copied", &totals.ram),
        (Test::Network, "Network bytes received", &totals.network),
        (
            Test::Storage,
            "Storage bytes verified",
            &totals.storage_verified,
        ),
    ] {
        if plan.includes(test) && (test != Test::Storage || plan.storage.is_some()) {
            if test == Test::Storage {
                report_line!("Storage bytes written: {}", totals.storage_written);
            }
            report_line!(
                "{name}: {} (measured {:.3} s)",
                sample.count,
                sample.elapsed.as_secs_f64()
            );
            let seconds = sample.elapsed.as_secs_f64();
            if seconds > 0.0 {
                let (label, quantity, unit) = match test {
                    Test::Cpu => ("Stress CPU throughput", sample.count as f64, "iterations/s"),
                    Test::Ram => (
                        "Stress RAM copy throughput",
                        sample.count as f64 / crate::common::MIB as f64,
                        "MiB/s",
                    ),
                    Test::Network => (
                        "Stress TCP loopback throughput",
                        sample.count as f64 / crate::common::MIB as f64,
                        "MiB/s",
                    ),
                    Test::Storage => (
                        "Stress storage verified round-trip throughput",
                        sample.count as f64 / crate::common::MIB as f64,
                        "MiB/s",
                    ),
                };
                crate::report::metric(label, quantity / seconds, unit);
            }
        }
    }
}

fn metrics_json(plan: &Plan, totals: &Totals) -> String {
    let rows = [
        (Test::Cpu, "iterations", &totals.cpu),
        (Test::Ram, "bytes_copied", &totals.ram),
        (Test::Network, "loopback_bytes_received", &totals.network),
        (Test::Storage, "bytes_verified", &totals.storage_verified),
    ];
    let mut metrics = Vec::new();
    for (test, unit, sample) in rows {
        if plan.includes(test) && (test != Test::Storage || plan.storage.is_some()) {
            metrics.push(format!(
                r#""{}":{{"unit":"{unit}","count":"{}","measured_ns":"{}"}}"#,
                test.name(),
                sample.count,
                sample.elapsed.as_nanos()
            ));
        }
    }
    format!("{{{}}}", metrics.join(","))
}

pub(crate) fn run(plan: Plan) -> Result<(), String> {
    let mut summary = plan
        .summary_path
        .as_ref()
        .map(|path| {
            OpenOptions::new()
                .write(true)
                .mode(0o600)
                .create_new(true)
                .open(path)
        })
        .transpose()
        .map_err(|e| format!("cannot create stress summary (choose a new path): {e}"))?;
    crate::report::identify(crate::report::Subject::Stress {
        test: plan.test,
        storage: plan.storage.is_some(),
    });
    let (target, fixed) = match plan.schedule {
        Schedule::Rounds { count, timeout } => (timeout, Some(count)),
        Schedule::Timed(target) => (target, None),
    };
    let start = Instant::now();
    let deadline = Deadline::after(target);
    let workers = Workers::new(&plan)?;
    let mut probes = plan
        .test
        .is_none()
        .then(|| probes::Probes::discover(deadline));
    report_line!("Stress auto (concurrent load)");
    if let Some(rounds) = fixed {
        report_line!("Rounds: {rounds}\nTime limit: {} ms", target.as_millis());
    } else {
        report_line!("Duration target: {} s", target.as_secs());
    }
    describe_tests(&plan);

    let mut totals = Totals::default();
    let mut rounds = 0u64;
    let mut partial = 0u64;
    let mut last_progress = start;
    let result = loop {
        if fixed == Some(rounds) {
            break Ok("round target reached");
        }
        if deadline.check().is_err() {
            break if fixed.is_some() {
                Err("time limit reached".into())
            } else {
                Ok("duration reached")
            };
        }
        let next_round = rounds + 1;
        if let Some(total) = fixed {
            println!("Round {next_round}/{total}");
        }
        match run_round(
            &plan,
            &workers,
            &mut probes,
            &mut totals,
            deadline,
            fixed.map(|total| (next_round, total)),
        ) {
            Ok(true) => rounds += 1,
            Ok(false) if fixed.is_none() => {
                partial = 1;
                break Ok("duration reached");
            }
            Ok(false) => break Err(format!("stress round {next_round}: time limit reached")),
            Err(error) => break Err(format!("stress round {next_round}: {error}")),
        }
        if fixed.is_none()
            && plan.test == Some(Test::Storage)
            && plan.storage.as_ref().map(|storage| storage.total_bytes)
                == Some(totals.storage_written)
        {
            break Ok("storage write budget reached");
        }
        if fixed.is_none() && last_progress.elapsed() >= PROGRESS_INTERVAL {
            println!(
                "Progress: {} s elapsed, {rounds} rounds completed",
                start.elapsed().as_secs()
            );
            last_progress = Instant::now();
        }
    };

    report_line!("Elapsed: {:.6} s", start.elapsed().as_secs_f64());
    if let Some(probes) = &probes {
        report_line!("Passive observations: {}", probes.describe());
    }
    if let Some(expected) = fixed {
        describe_totals(&plan, &totals);
        if result.is_ok() {
            report_line!("Stress result: PASS ({rounds}/{expected} rounds completed)");
        } else {
            report_line!("Stress result: FAIL ({rounds}/{expected} rounds completed)");
        }
    } else {
        report_line!("Rounds completed: {rounds}\nPartial rounds: {partial}");
        describe_totals(&plan, &totals);
        match &result {
            Ok(reason) => report_line!("Stop reason: {reason}\nStress result: PASS"),
            Err(_) => report_line!("Stress result: FAIL"),
        }
    }
    if let Some(file) = summary.as_mut() {
        let target_rounds = fixed.map_or_else(|| "null".to_owned(), |n| n.to_string());
        let (status, reason) = match &result {
            Ok("round target reached") => ("completed", "round_target"),
            Ok("duration reached") => ("completed", "duration_limit"),
            Ok("storage write budget reached") => ("completed", "storage_budget"),
            Ok(_) => ("completed", "other"),
            Err(_) => ("failed", "execution_error"),
        };
        let storage_written = plan.storage.as_ref().map_or_else(
            || "null".to_owned(),
            |_| format!(r#""{}""#, totals.storage_written),
        );
        let write_result = (|| -> std::io::Result<()> {
            writeln!(
                file,
                r#"{{"summary_schema_version":1,"kind":"stress_auto","status":"{status}","stop_reason":"{reason}","target_rounds":{target_rounds},"rounds_completed":{rounds},"final_partial_rounds":{partial},"elapsed_monotonic_ms":{},"storage_bytes_written":{storage_written},"workload_metrics":{}}}"#,
                start.elapsed().as_millis(),
                metrics_json(&plan, &totals)
            )?;
            file.sync_all()
        })();
        if let Err(error) = write_result {
            let prefix = result
                .as_ref()
                .err()
                .map_or(String::new(), |e| format!("{e}; "));
            return Err(format!("{prefix}cannot save stress summary: {error}"));
        }
    }
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_keeps_wide_counts_and_durations_exact() {
        let plan = Plan {
            test: Some(Test::Cpu),
            schedule: Schedule::Timed(Duration::from_secs(1)),
            duration: Duration::from_millis(15),
            buffer_kib: 4,
            network_bytes: MIB,
            storage: None,
            summary_path: None,
        };
        let totals = Totals {
            cpu: Sample {
                count: u64::MAX,
                elapsed: Duration::from_nanos(9_007_199_254_740_993),
            },
            ..Totals::default()
        };
        assert_eq!(
            metrics_json(&plan, &totals),
            r#"{"cpu":{"unit":"iterations","count":"18446744073709551615","measured_ns":"9007199254740993"}}"#
        );
    }
}

use crate::common::{Deadline, RunError, Sample, MIB};
use crate::{cpu, network, ram, storage};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_ROUNDS: u64 = 3;
pub(crate) const MAX_ROUNDS: u64 = 5;
pub(crate) const DEFAULT_DURATION_MS: u64 = 250;
pub(crate) const MAX_DURATION_MS: u64 = 5_000;
pub(crate) const DEFAULT_BUFFER_KIB: u64 = 1_024;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 120_000;
pub(crate) const MAX_DURATION_SECS: u64 = 14 * 24 * 60 * 60;
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

    fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Ram => "RAM",
            Self::Network => "Network",
            Self::Storage => "Storage",
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
}

pub(crate) enum Schedule {
    Rounds { count: u64, timeout: Duration },
    Timed(Duration),
}

impl Plan {
    fn tests(&self) -> impl Iterator<Item = Test> + '_ {
        [Test::Cpu, Test::Ram, Test::Network, Test::Storage]
            .into_iter()
            .filter(|&test| {
                self.test.map_or(true, |selected| selected == test)
                    && (test != Test::Storage || self.storage.is_some())
            })
    }
}

fn phase(
    name: &str,
    deadline: Deadline,
    run: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let result = deadline
        .check()
        .map_err(String::from)
        .and_then(|()| run())
        .and_then(|()| deadline.check().map_err(String::from));
    match result {
        Ok(()) => {
            println!("{name}: PASS");
            Ok(())
        }
        Err(error) => {
            println!("{name}: FAIL");
            Err(format!("{name}: {error}"))
        }
    }
}

fn run_round(plan: &Plan, round: u64, rounds: u64, deadline: Deadline) -> Result<(), String> {
    for test in plan.tests() {
        phase(test.label(), deadline, || match test {
            Test::Cpu => cpu::run(plan.duration, Some(deadline)),
            Test::Ram => ram::run(plan.duration, plan.buffer_kib, Some(deadline)),
            Test::Network => network::run(plan.network_bytes, deadline),
            Test::Storage => {
                let storage = plan.storage.as_ref().expect("storage is enabled");
                let mut bytes = storage.total_bytes / rounds;
                if round == rounds {
                    bytes += storage.total_bytes % rounds;
                }
                storage::run(&storage.directory, bytes, storage.sync, deadline)
            }
        })?;
    }
    Ok(())
}

fn describe_tests(plan: &Plan) {
    if let Some(test) = plan.test {
        println!("Test: {}", test.name());
    }
    match (&plan.storage, plan.test) {
        (Some(storage), _) => println!(
            "Storage write budget: {} bytes across all rounds",
            storage.total_bytes
        ),
        (None, None) => println!("Storage: disabled"),
        (None, Some(_)) => (),
    }
}

fn run_rounds(plan: &Plan, rounds: u64, timeout: Duration) -> Result<(), String> {
    let deadline = Deadline::after(timeout);
    println!(
        "Stress auto (sequential)\nRounds: {rounds}\nTime limit: {} ms",
        timeout.as_millis()
    );
    describe_tests(plan);
    for round in 1..=rounds {
        println!("Round {round}/{rounds}");
        if let Err(error) = run_round(plan, round, rounds, deadline) {
            println!(
                "Stress result: FAIL ({}/{} rounds completed)",
                round - 1,
                rounds
            );
            return Err(format!("stress round {round}: {error}"));
        }
    }
    println!("Stress result: PASS ({rounds}/{rounds} rounds completed)");
    Ok(())
}

#[derive(Default)]
struct Totals {
    cpu: Sample,
    ram: Sample,
    network: Sample,
    storage_written: u64,
    storage_verified: Sample,
}

fn timed_storage(
    storage: &Storage,
    totals: &mut Totals,
    deadline: Deadline,
) -> Result<(), RunError> {
    let remaining = storage.total_bytes - totals.storage_written;
    if remaining == 0 {
        return Ok(());
    }
    let bytes = remaining.min(STORAGE_CHUNK_BYTES);
    let result = storage::benchmark(
        &storage.directory,
        bytes,
        storage.sync,
        deadline,
        &mut totals.storage_written,
    );
    if totals.storage_written == storage.total_bytes {
        println!(
            "Storage: write budget reached ({} bytes)",
            storage.total_bytes
        );
    }
    let (write, read) = result?;
    totals.storage_verified.add(Sample {
        count: bytes,
        elapsed: write + read,
    });
    Ok(())
}

fn timed_round(plan: &Plan, totals: &mut Totals, deadline: Deadline) -> Result<bool, String> {
    for test in plan.tests() {
        if deadline.check().is_err() {
            return Ok(false);
        }
        let operation_deadline = match test {
            Test::Network => deadline.capped(Duration::from_millis(network::DEFAULT_TIMEOUT_MS)),
            Test::Storage => deadline.capped(Duration::from_millis(storage::DEFAULT_TIMEOUT_MS)),
            Test::Cpu | Test::Ram => deadline,
        };
        let result = match test {
            Test::Cpu => cpu::benchmark(plan.duration, Some(operation_deadline))
                .map(|(sample, _)| totals.cpu.add(sample)),
            Test::Ram => ram::benchmark(plan.duration, plan.buffer_kib, Some(operation_deadline))
                .map(|sample| totals.ram.add(sample)),
            Test::Network => network::benchmark(plan.network_bytes, operation_deadline)
                .map(|sample| totals.network.add(sample)),
            Test::Storage => timed_storage(
                plan.storage.as_ref().expect("storage is enabled"),
                totals,
                operation_deadline,
            ),
        };
        match result {
            Ok(()) => (),
            Err(RunError::Deadline) if operation_deadline == deadline => return Ok(false),
            Err(error) => return Err(format!("{}: {error}", test.label())),
        }
    }
    Ok(true)
}

fn run_timed(plan: &Plan, target: Duration) -> Result<(), String> {
    let start = Instant::now();
    let deadline = Deadline::after(target);
    println!(
        "Stress auto (sequential)\nDuration target: {} s",
        target.as_secs()
    );
    describe_tests(plan);

    let mut totals = Totals::default();
    let mut rounds = 0u64;
    let mut last_progress = start;
    let stop = loop {
        match timed_round(plan, &mut totals, deadline) {
            Ok(true) => rounds = rounds.saturating_add(1),
            Ok(false) => break Ok("duration reached"),
            Err(error) => break Err(error),
        }
        if plan.test == Some(Test::Storage)
            && plan.storage.as_ref().map(|storage| storage.total_bytes)
                == Some(totals.storage_written)
        {
            break Ok("storage write budget reached");
        }
        if last_progress.elapsed() >= PROGRESS_INTERVAL {
            println!(
                "Progress: {} s elapsed, {rounds} rounds completed",
                start.elapsed().as_secs()
            );
            last_progress = Instant::now();
        }
    };

    println!(
        "Elapsed: {:.3} s\nRounds completed: {rounds}",
        start.elapsed().as_secs_f64()
    );
    for test in plan.tests() {
        let (name, sample) = match test {
            Test::Cpu => ("CPU iterations", &totals.cpu),
            Test::Ram => ("RAM bytes copied", &totals.ram),
            Test::Network => ("Network bytes received", &totals.network),
            Test::Storage => {
                println!("Storage bytes written: {}", totals.storage_written);
                ("Storage bytes verified", &totals.storage_verified)
            }
        };
        println!(
            "{name}: {} (measured {:.3} s)",
            sample.count,
            sample.elapsed.as_secs_f64()
        );
    }
    match stop {
        Ok(reason) => {
            println!("Stop reason: {reason}\nStress result: PASS");
            Ok(())
        }
        Err(error) => {
            println!("Stress result: FAIL");
            Err(error)
        }
    }
}

pub(crate) fn run(plan: Plan) -> Result<(), String> {
    match plan.schedule {
        Schedule::Rounds { count, timeout } => run_rounds(&plan, count, timeout),
        Schedule::Timed(target) => run_timed(&plan, target),
    }
}

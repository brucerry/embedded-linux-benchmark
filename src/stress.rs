use crate::common::Deadline;
use crate::{cpu, network, ram, storage};
use std::path::PathBuf;
use std::time::Duration;

pub(crate) const DEFAULT_ROUNDS: u64 = 3;
pub(crate) const MAX_ROUNDS: u64 = 5;
pub(crate) const DEFAULT_DURATION_MS: u64 = 250;
pub(crate) const MAX_DURATION_MS: u64 = 5_000;
pub(crate) const DEFAULT_BUFFER_KIB: u64 = 1_024;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 15_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 120_000;

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
    pub(crate) rounds: u64,
    pub(crate) duration: Duration,
    pub(crate) buffer_kib: u64,
    pub(crate) network_bytes: u64,
    pub(crate) timeout: Duration,
    pub(crate) storage: Option<Storage>,
}

fn phase(
    name: &str,
    deadline: Deadline,
    run: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let result = deadline
        .check()
        .and_then(|()| run())
        .and_then(|()| deadline.check());
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

fn run_round(plan: &Plan, round: u64, deadline: Deadline) -> Result<(), String> {
    if matches!(plan.test, None | Some(Test::Cpu)) {
        phase("CPU", deadline, || cpu::run(plan.duration, Some(deadline)))?;
    }
    if matches!(plan.test, None | Some(Test::Ram)) {
        phase("RAM", deadline, || {
            ram::run(plan.duration, plan.buffer_kib, Some(deadline))
        })?;
    }
    if matches!(plan.test, None | Some(Test::Network)) {
        phase("Network", deadline, || {
            network::run(plan.network_bytes, deadline)
        })?;
    }
    if let Some(storage) = plan
        .storage
        .as_ref()
        .filter(|_| matches!(plan.test, None | Some(Test::Storage)))
    {
        let mut bytes = storage.total_bytes / plan.rounds;
        if round == plan.rounds {
            bytes += storage.total_bytes % plan.rounds;
        }
        phase("Storage", deadline, || {
            storage::run(&storage.directory, bytes, storage.sync, deadline)
        })?;
    }
    Ok(())
}

pub(crate) fn run(plan: Plan) -> Result<(), String> {
    let deadline = Deadline::after(plan.timeout);
    println!(
        "Stress auto (sequential)\nRounds: {}\nTime limit: {} ms",
        plan.rounds,
        plan.timeout.as_millis()
    );
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
    for round in 1..=plan.rounds {
        println!("Round {round}/{}", plan.rounds);
        if let Err(error) = run_round(&plan, round, deadline) {
            println!(
                "Stress result: FAIL ({}/{} rounds completed)",
                round - 1,
                plan.rounds
            );
            return Err(format!("stress round {round}: {error}"));
        }
    }
    println!(
        "Stress result: PASS ({}/{} rounds completed)",
        plan.rounds, plan.rounds
    );
    Ok(())
}

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

pub(crate) struct Storage {
    pub(crate) directory: PathBuf,
    pub(crate) total_bytes: u64,
    pub(crate) sync: bool,
}

pub(crate) struct Plan {
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
    phase("CPU", deadline, || cpu::run(plan.duration, Some(deadline)))?;
    phase("RAM", deadline, || {
        ram::run(plan.duration, plan.buffer_kib, Some(deadline))
    })?;
    phase("Network", deadline, || {
        network::run(plan.network_bytes, deadline)
    })?;
    if let Some(storage) = &plan.storage {
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
    match &plan.storage {
        Some(storage) => println!(
            "Storage write budget: {} bytes across all rounds",
            storage.total_bytes
        ),
        None => println!("Storage: disabled"),
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

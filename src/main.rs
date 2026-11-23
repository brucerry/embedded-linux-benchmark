mod cli;
mod common;
mod cpu;
mod gpio;
mod network;
mod ram;
mod storage;
mod stress;
mod uart;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

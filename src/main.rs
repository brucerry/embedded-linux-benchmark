macro_rules! report_line {
    ($($arg:tt)*) => { $crate::report::line(format_args!($($arg)*)) };
}

mod camera;
mod can;
mod cli;
mod common;
mod cpu;
mod gpio;
mod gpio_chardev;
mod hdmi;
mod i2c;
mod iio;
mod linux_poll;
mod linux_socket;
mod network;
mod network_peer;
mod pcie;
mod peripheral;
mod ram;
mod report;
mod spi;
mod storage;
mod stress;
mod sysfs;
mod uart;
mod usb;

use std::process::ExitCode;

fn main() -> ExitCode {
    report::begin();
    let result = cli::run();
    match report::finish(&result) {
        Ok(Some(directory)) => {
            println!("Report: {}", directory.join("report.md").display());
            println!("Plots: {}", directory.join("plots.pdf").display());
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!("Error: could not write benchmark report under /tmp: {error}");
            if result.is_ok() {
                return ExitCode::FAILURE;
            }
        }
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

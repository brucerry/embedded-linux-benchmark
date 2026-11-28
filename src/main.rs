macro_rules! report_line {
    ($($arg:tt)*) => { $crate::report::line(format_args!($($arg)*)) };
}

mod accelerator;
mod accelerator_interfaces;
mod accelerator_telemetry;
mod camera;
mod can;
mod cli;
mod common;
mod cpu;
mod edac;
mod gpio;
mod gpio_chardev;
mod hdmi;
mod hugepage_pool;
mod hwmon;
mod i2c;
mod iio;
mod ina3221;
mod inventory;
mod linux_poll;
mod linux_socket;
mod mmc_health;
mod network;
mod network_health;
mod network_peer;
mod numa;
mod nvidia_capture;
mod nvme_health;
mod pcie;
mod pcie_health;
mod pcie_locality;
mod peripheral;
mod pressure;
mod qualification;
mod ram;
mod ras_capture;
mod rdma_port;
mod report;
mod spi;
mod storage;
mod stress;
mod sysfs;
mod tegrastats_capture;
mod thermal_cooling;
mod thermal_throttle;
mod uart;
mod usb;

use std::process::ExitCode;

fn main() -> ExitCode {
    report::begin();
    let result = cli::run();
    match report::finish(&result) {
        Ok(Some(files)) => {
            println!("Report: {}", files.markdown.display());
            println!("Plots: {}", files.pdf.display());
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

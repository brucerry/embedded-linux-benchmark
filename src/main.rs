mod camera;
mod can;
mod cli;
mod common;
mod cpu;
mod gpio;
mod gpio_chardev;
mod hdmi;
mod i2c;
mod linux_poll;
mod linux_socket;
mod network;
mod network_peer;
mod peripheral;
mod ram;
mod spi;
mod storage;
mod stress;
mod uart;
mod usb;

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

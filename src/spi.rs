use crate::common::Deadline;
use crate::peripheral::{self, Kind};
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_BYTES: u64 = 4096;
pub(crate) const MAX_BYTES: u64 = 65_536;
pub(crate) const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;
const CHUNK: usize = 1024;

// Allow standard clock modes, chip-select polarity, LSB first, and no CS.
const SPI_CPHA: u32 = 1 << 0;
const SPI_CPOL: u32 = 1 << 1;
const SPI_CS_HIGH: u32 = 1 << 2;
const SPI_LSB_FIRST: u32 = 1 << 3;
const SPI_NO_CS: u32 = 1 << 6;
const ALLOWED_MODE_FLAGS: u32 = SPI_CPHA | SPI_CPOL | SPI_CS_HIGH | SPI_LSB_FIRST | SPI_NO_CS;

#[repr(C)]
#[derive(Default)]
struct Transfer {
    tx_buf: u64,
    rx_buf: u64,
    len: u32,
    speed_hz: u32,
    delay_usecs: u16,
    bits_per_word: u8,
    cs_change: u8,
    tx_nbits: u8,
    rx_nbits: u8,
    word_delay_usecs: u8,
    pad: u8,
}

const _: [(); 32] = [(); std::mem::size_of::<Transfer>()];
const MESSAGE: u32 = peripheral::ioctl_write::<Transfer>(b'k', 0);
const READ_BITS_PER_WORD: u32 = peripheral::ioctl_read::<u8>(b'k', 3);
const READ_MAX_SPEED_HZ: u32 = peripheral::ioctl_read::<u32>(b'k', 4);
const READ_MODE32: u32 = peripheral::ioctl_read::<u32>(b'k', 5);

fn exchange(file: &File, tx: &[u8], rx: &mut [u8]) -> io::Result<usize> {
    let mut transfer = Transfer {
        tx_buf: tx.as_ptr() as usize as u64,
        rx_buf: rx.as_mut_ptr() as usize as u64,
        len: tx.len() as u32,
        ..Transfer::default()
    };
    // Both buffers are at least len bytes and remain alive during SPI_IOC_MESSAGE(1).
    // Zero overrides inherit the device's speed and word length.
    unsafe { peripheral::ioctl_ptr(file, MESSAGE, &mut transfer) }
}

fn loopback(
    bytes: usize,
    deadline: Deadline,
    mut exchange: impl FnMut(&[u8], &mut [u8]) -> io::Result<usize>,
) -> Result<Duration, String> {
    let mut tx = [0; CHUNK];
    let mut rx = [0; CHUNK];
    let mut pattern = 0x1234_5678u32;
    let start = Instant::now();
    for offset in (0..bytes).step_by(CHUNK) {
        deadline.check()?;
        let length = CHUNK.min(bytes - offset);
        for (sent, received) in tx[..length].iter_mut().zip(&mut rx[..length]) {
            pattern ^= pattern << 13;
            pattern ^= pattern >> 17;
            pattern ^= pattern << 5;
            *sent = pattern as u8;
            *received = !*sent;
        }
        let count = exchange(&tx[..length], &mut rx[..length])
            .map_err(|error| format!("SPI full-duplex transfer failed: {error}"))?;
        if count != length {
            return Err(format!("SPI transfer returned {count} of {length} bytes"));
        }
        if let Some(mismatch) = tx[..length]
            .iter()
            .zip(&rx[..length])
            .position(|(sent, received)| sent != received)
        {
            return Err(format!(
                "SPI loopback payload mismatch at byte {}",
                offset + mismatch
            ));
        }
    }
    deadline.check()?;
    Ok(start.elapsed())
}

fn existing_config(file: &File) -> Result<(u32, u32), String> {
    let mut mode = 0u32;
    let mut bits = 0u8;
    let mut speed_hz = 0u32;
    // The spidev read-only ioctls write the scalar declared by their UAPI.
    unsafe {
        peripheral::ioctl_ptr(file, READ_MODE32, &mut mode)
            .map_err(|error| format!("cannot read SPI mode: {error}"))?;
        peripheral::ioctl_ptr(file, READ_BITS_PER_WORD, &mut bits)
            .map_err(|error| format!("cannot read SPI word length: {error}"))?;
        peripheral::ioctl_ptr(file, READ_MAX_SPEED_HZ, &mut speed_hz)
            .map_err(|error| format!("cannot read SPI configured maximum speed: {error}"))?;
    }
    if mode & !ALLOWED_MODE_FLAGS != 0 || bits != 8 || speed_hz == 0 {
        return Err(format!(
            "SPI loopback requires an existing 8-bit single-lane full-duplex mode and nonzero speed (flags 0x{mode:x}, {bits} bits, {speed_hz} Hz)"
        ));
    }
    Ok((mode, speed_hz))
}

pub(crate) fn run(device: &Path, bytes: usize, deadline: Deadline) -> Result<(), String> {
    deadline.check()?;
    let file = peripheral::open_device(device, Kind::Spi, true)?;
    let (mode, speed_hz) = existing_config(&file)?;
    let elapsed = loopback(bytes, deadline, |tx, rx| exchange(&file, tx, rx))?;
    crate::report::metric(
        "SPI loopback throughput",
        bytes as f64 / elapsed.as_secs_f64(),
        "B/s",
    );
    report_line!("SPI throughput (verified full-duplex loopback)");
    report_line!("Device: {}", device.display());
    report_line!("Configured mode flags: 0x{mode:x}");
    report_line!("Configured maximum speed: {speed_hz} Hz");
    report_line!("Bytes verified: {bytes}");
    report_line!("Elapsed: {:.6} s", elapsed.as_secs_f64());
    report_line!(
        "Throughput: {:.2} B/s",
        bytes as f64 / elapsed.as_secs_f64()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_duplex_chunks_verify_receive_count_and_data() {
        let deadline = || Deadline::after(Duration::from_secs(5));
        let mut lengths = Vec::new();
        loopback(2051, deadline(), |tx, rx| {
            lengths.push(tx.len());
            rx.copy_from_slice(tx);
            Ok(rx.len())
        })
        .unwrap();
        assert_eq!(lengths, [1024, 1024, 3]);
        assert!(loopback(12, deadline(), |_, _| Ok(11))
            .unwrap_err()
            .contains("11 of 12"));
        assert!(loopback(12, deadline(), |tx, rx| {
            rx.copy_from_slice(tx);
            rx[2] ^= 1;
            Ok(12)
        })
        .unwrap_err()
        .contains("byte 2"));
        assert!(loopback(1, Deadline::after(Duration::ZERO), |_, _| {
            panic!("transaction after deadline")
        })
        .unwrap_err()
        .contains("time limit"));
    }

    #[test]
    fn spi_abi_requests_match_linux_headers() {
        #[cfg(not(target_arch = "mips"))]
        assert_eq!(
            (MESSAGE, READ_BITS_PER_WORD, READ_MAX_SPEED_HZ, READ_MODE32),
            (0x40206b00, 0x80016b03, 0x80046b04, 0x80046b05)
        );
        #[cfg(target_arch = "mips")]
        assert_eq!(
            (MESSAGE, READ_BITS_PER_WORD, READ_MAX_SPEED_HZ, READ_MODE32),
            (0x80206b00, 0x40016b03, 0x40046b04, 0x40046b05)
        );
        let transfer = Transfer::default();
        let base = &transfer as *const Transfer as usize;
        assert_eq!(&transfer.len as *const u32 as usize - base, 16);
        assert_eq!(&transfer.bits_per_word as *const u8 as usize - base, 26);
        assert_eq!(&transfer.pad as *const u8 as usize - base, 31);
    }
}

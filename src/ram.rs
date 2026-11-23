use crate::common::{measure, Deadline, RunError, Sample, MIB};
use std::hint::black_box;
use std::time::Duration;

pub(crate) const DEFAULT_BUFFER_KIB: u64 = 4_096;
pub(crate) const MIN_BUFFER_KIB: u64 = 4;
pub(crate) const MAX_BUFFER_KIB: u64 = 32_768;

fn allocate_buffer(size: usize, byte: u8, label: &str) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(size)
        .map_err(|error| format!("could not allocate {label} buffer ({size} bytes): {error}"))?;
    buffer.resize(size, byte);
    Ok(buffer)
}

pub(crate) fn benchmark(
    duration: Duration,
    buffer_kib: u64,
    deadline: Option<Deadline>,
) -> Result<Sample, RunError> {
    let buffer_bytes = (buffer_kib * 1_024) as usize;
    let mut source = allocate_buffer(buffer_bytes, 0xa5, "source")?;
    for (index, byte) in source.iter_mut().enumerate() {
        *byte = byte.wrapping_add(index as u8);
    }
    let mut destination = allocate_buffer(buffer_bytes, 0x5a, "destination")?;
    let mut copy = || {
        black_box(destination.as_mut_slice()).copy_from_slice(black_box(source.as_slice()));
        black_box(destination.as_slice());
    };
    copy();
    let mut sample = measure(duration, deadline, copy)?;
    if destination != source {
        return Err(RunError::Failed("RAM copy verification failed".into()));
    }
    sample.count *= buffer_bytes as u64;
    Ok(sample)
}

pub(crate) fn run(
    duration: Duration,
    buffer_kib: u64,
    deadline: Option<Deadline>,
) -> Result<(), String> {
    let sample = benchmark(duration, buffer_kib, deadline)?;
    if sample.elapsed < duration {
        return Err(RunError::Deadline.into());
    }
    let bytes_copied = sample.count;
    let copies = bytes_copied / (buffer_kib * 1_024);
    let seconds = sample.elapsed.as_secs_f64();
    println!(
        "RAM copy throughput (single thread)\n\
         Buffer size: {buffer_kib} KiB each\n\
         Working set: {} KiB (source + destination)\n\
         Elapsed: {seconds:.6} s\n\
         Copies: {copies}\n\
         Bytes copied: {bytes_copied}\n\
         Throughput: {:.2} MiB/s",
        buffer_kib * 2,
        bytes_copied as f64 / (MIB as f64 * seconds),
    );
    Ok(())
}

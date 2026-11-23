use crate::common::{measure, Deadline, RunError, Sample};
use std::hint::black_box;
use std::time::Duration;

const CPU_BATCH_SIZE: u64 = 4_096;
const CPU_SEED: u32 = 0x1234_5678;

fn xorshift_batch(mut state: u32) -> u32 {
    for _ in 0..CPU_BATCH_SIZE {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
    }
    state
}

pub(crate) fn benchmark(
    duration: Duration,
    deadline: Option<Deadline>,
) -> Result<(Sample, u32), RunError> {
    black_box(xorshift_batch(black_box(CPU_SEED)));
    let mut state = black_box(CPU_SEED);
    let mut sample = measure(duration, deadline, || state = xorshift_batch(state))?;
    sample.count *= CPU_BATCH_SIZE;
    Ok((sample, state))
}

pub(crate) fn run(duration: Duration, deadline: Option<Deadline>) -> Result<(), String> {
    let (sample, state) = benchmark(duration, deadline)?;
    if sample.elapsed < duration {
        return Err(RunError::Deadline.into());
    }
    let iterations = sample.count;
    let seconds = sample.elapsed.as_secs_f64();
    println!(
        "CPU throughput (single thread, 32-bit xorshift)\n\
         Elapsed: {seconds:.6} s\n\
         Iterations: {iterations}\n\
         Throughput: {:.2} iterations/s\n\
         Checksum: {state:08x}",
        iterations as f64 / seconds,
    );
    Ok(())
}

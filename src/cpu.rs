use crate::common::{measure, Deadline};
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

pub(crate) fn run(duration: Duration, deadline: Option<Deadline>) -> Result<(), String> {
    black_box(xorshift_batch(black_box(CPU_SEED)));
    let mut state = black_box(CPU_SEED);
    let (batches, elapsed) = measure(duration, deadline, || state = xorshift_batch(state))?;
    let iterations = batches * CPU_BATCH_SIZE;
    let seconds = elapsed.as_secs_f64();
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

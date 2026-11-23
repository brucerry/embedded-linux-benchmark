use std::env;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const DEFAULT_DURATION_MS: u64 = 1_000;
const MAX_DURATION_MS: u64 = 60_000;
const DEFAULT_BUFFER_KIB: u64 = 4_096;
const MIN_BUFFER_KIB: u64 = 4;
const MAX_BUFFER_KIB: u64 = 32_768;
const CPU_BATCH_SIZE: u64 = 4_096;
const CPU_SEED: u32 = 0x1234_5678;

enum Benchmark {
    Cpu { duration: Duration },
    Ram { duration: Duration, buffer_kib: u64 },
}

fn main() -> ExitCode {
    match parse_args().and_then(|benchmark| match benchmark {
        Some(Benchmark::Cpu { duration }) => {
            run_cpu(duration);
            Ok(())
        }
        Some(Benchmark::Ram {
            duration,
            buffer_kib,
        }) => run_ram(duration, buffer_kib),
        None => {
            print_help();
            Ok(())
        }
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    println!(
        "Usage:\n  embedded-linux-benchmark cpu [--duration-ms MS]\n  \
         embedded-linux-benchmark ram [--duration-ms MS] [--buffer-kib KIB]\n\n\
         cpu  Measure single-threaded 32-bit integer throughput\n\
         ram  Measure copy throughput between two equal-sized buffers\n\n\
         --duration-ms  Measurement time in milliseconds (1–{MAX_DURATION_MS}; default {DEFAULT_DURATION_MS})\n\
         --buffer-kib   Size of each RAM buffer in KiB ({MIN_BUFFER_KIB}–{MAX_BUFFER_KIB}; default {DEFAULT_BUFFER_KIB})\n\
         -h, --help     Show this help"
    );
}

fn parse_args() -> Result<Option<Benchmark>, String> {
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().map_err(|_| "arguments must be UTF-8"))
        .collect::<Result<_, _>>()?;
    let (command, options) = match args.split_first() {
        None => return Ok(None),
        Some((command, options)) => (command.as_str(), options),
    };
    let ram = match command {
        "cpu" => false,
        "ram" => true,
        "-h" | "--help" if options.is_empty() => return Ok(None),
        _ => return Err(format!("unknown command '{command}'. See --help.")),
    };
    if matches!(options, [help] if help == "--help" || help == "-h") {
        return Ok(None);
    }

    let mut duration_ms = None;
    let mut buffer_kib = None;
    let mut options = options.iter();
    while let Some(option) = options.next() {
        let (setting, min, max) = match option.as_str() {
            "--duration-ms" => (&mut duration_ms, 1, MAX_DURATION_MS),
            "--buffer-kib" if ram => (&mut buffer_kib, MIN_BUFFER_KIB, MAX_BUFFER_KIB),
            _ => {
                return Err(format!(
                    "unknown option '{option}' for {command}. See --help."
                ))
            }
        };
        if setting.is_some() {
            return Err(format!("option '{option}' may only be set once"));
        }
        let value = options
            .next()
            .filter(|value| !value.starts_with("--"))
            .ok_or_else(|| format!("missing value for '{option}'"))?;
        *setting = Some(
            value
                .parse::<u64>()
                .ok()
                .filter(|value| (min..=max).contains(value))
                .ok_or_else(|| format!("'{option}' must be an integer between {min} and {max}"))?,
        );
    }

    let duration = Duration::from_millis(duration_ms.unwrap_or(DEFAULT_DURATION_MS));
    Ok(Some(if ram {
        Benchmark::Ram {
            duration,
            buffer_kib: buffer_kib.unwrap_or(DEFAULT_BUFFER_KIB),
        }
    } else {
        Benchmark::Cpu { duration }
    }))
}

fn measure(minimum: Duration, mut work: impl FnMut()) -> (u64, Duration) {
    let start = Instant::now();
    let mut rounds = 0;
    loop {
        work();
        rounds += 1;
        let elapsed = start.elapsed();
        if elapsed >= minimum {
            return (rounds, elapsed);
        }
    }
}

fn xorshift_batch(mut state: u32) -> u32 {
    for _ in 0..CPU_BATCH_SIZE {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
    }
    state
}

fn run_cpu(duration: Duration) {
    black_box(xorshift_batch(black_box(CPU_SEED)));
    let mut state = black_box(CPU_SEED);
    let (batches, elapsed) = measure(duration, || state = xorshift_batch(state));
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
}

fn allocate_buffer(size: usize, byte: u8, label: &str) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(size)
        .map_err(|error| format!("could not allocate {label} buffer ({size} bytes): {error}"))?;
    buffer.resize(size, byte);
    Ok(buffer)
}

fn run_ram(duration: Duration, buffer_kib: u64) -> Result<(), String> {
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
    let (copies, elapsed) = measure(duration, copy);
    if destination != source {
        return Err("RAM copy verification failed".into());
    }

    let bytes_copied = copies * buffer_bytes as u64;
    let seconds = elapsed.as_secs_f64();
    println!(
        "RAM copy throughput (single thread)\n\
         Buffer size: {buffer_kib} KiB each\n\
         Working set: {} KiB (source + destination)\n\
         Elapsed: {seconds:.6} s\n\
         Copies: {copies}\n\
         Bytes copied: {bytes_copied}\n\
         Throughput: {:.2} MiB/s",
        buffer_kib * 2,
        bytes_copied as f64 / (1_048_576.0 * seconds),
    );
    Ok(())
}

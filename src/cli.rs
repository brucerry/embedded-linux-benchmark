use crate::common::{Deadline, DEFAULT_DURATION_MS, MAX_DURATION_MS, MIB};
use crate::{cpu, gpio, network, ram, storage, stress, uart};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Kind {
    Number(u64, u64),
    Path,
    Flag,
    StressTest,
}

use Kind::{Flag, Number};

const CPU_OPTIONS: &[(&str, Kind)] = &[("--duration-ms", Number(1, MAX_DURATION_MS))];
const RAM_OPTIONS: &[(&str, Kind)] = &[
    ("--duration-ms", Number(1, MAX_DURATION_MS)),
    (
        "--buffer-kib",
        Number(ram::MIN_BUFFER_KIB, ram::MAX_BUFFER_KIB),
    ),
];
const STORAGE_OPTIONS: &[(&str, Kind)] = &[
    ("--dir", Kind::Path),
    ("--size-mib", Number(1, storage::MAX_MIB)),
    ("--timeout-ms", Number(1, storage::MAX_TIMEOUT_MS)),
    ("--sync", Flag),
];
const NETWORK_OPTIONS: &[(&str, Kind)] = &[
    ("--size-mib", Number(1, network::MAX_MIB)),
    ("--timeout-ms", Number(1, network::MAX_TIMEOUT_MS)),
];
const UART_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Path),
    ("--loopback", Flag),
    ("--bytes", Number(1, uart::MAX_BYTES)),
    ("--timeout-ms", Number(1, uart::MAX_TIMEOUT_MS)),
];
const GPIO_OPTIONS: &[(&str, Kind)] = &[
    ("--pin", Number(0, u32::MAX as u64)),
    ("--samples", Number(1, gpio::MAX_SAMPLES)),
    ("--timeout-ms", Number(1, gpio::MAX_TIMEOUT_MS)),
];
const STRESS_OPTIONS: &[(&str, Kind)] = &[
    ("--test", Kind::StressTest),
    ("--rounds", Number(1, stress::MAX_ROUNDS)),
    ("--duration-secs", Number(1, stress::MAX_DURATION_SECS)),
    ("--duration-ms", Number(1, stress::MAX_DURATION_MS)),
    (
        "--buffer-kib",
        Number(ram::MIN_BUFFER_KIB, ram::MAX_BUFFER_KIB),
    ),
    ("--network-mib", Number(1, network::MAX_MIB)),
    ("--timeout-ms", Number(1, stress::MAX_TIMEOUT_MS)),
    ("--storage-dir", Kind::Path),
    ("--storage-mib", Number(1, storage::MAX_MIB)),
    ("--storage-sync", Flag),
];

enum Value<'a> {
    Number(u64),
    Path(&'a Path),
    Flag,
    StressTest(stress::Test),
}

struct Options<'a>(HashMap<&'a str, Value<'a>>);

impl<'a> Options<'a> {
    fn parse(command: &str, args: &'a [String], spec: &[(&str, Kind)]) -> Result<Self, String> {
        let mut values = HashMap::new();
        let mut args = args.iter();
        while let Some(option) = args.next() {
            let (_, kind) = spec
                .iter()
                .find(|(name, _)| *name == option.as_str())
                .ok_or_else(|| format!("unknown option '{option}' for {command}. See --help."))?;
            if values.contains_key(option.as_str()) {
                return Err(format!("option '{option}' may only be set once"));
            }
            if matches!(kind, Flag) {
                values.insert(option.as_str(), Value::Flag);
                continue;
            }
            let raw = args
                .next()
                .filter(|value| !value.starts_with("--"))
                .ok_or_else(|| format!("missing value for '{option}'"))?;
            let value = match kind {
                Number(min, max) => Value::Number(
                    raw.parse::<u64>()
                        .ok()
                        .filter(|value| (min..=max).contains(&value))
                        .ok_or_else(|| {
                            format!("'{option}' must be an integer between {min} and {max}")
                        })?,
                ),
                Kind::Path if raw.is_empty() => {
                    return Err(format!("'{option}' must be a nonempty path"))
                }
                Kind::Path => Value::Path(Path::new(raw)),
                Kind::StressTest => Value::StressTest(match raw.as_str() {
                    "cpu" => stress::Test::Cpu,
                    "ram" => stress::Test::Ram,
                    "network" => stress::Test::Network,
                    "storage" => stress::Test::Storage,
                    _ => {
                        return Err(format!(
                            "invalid stress test '{raw}'. Choose cpu, ram, network, or storage."
                        ))
                    }
                }),
                Flag => unreachable!(),
            };
            values.insert(option.as_str(), value);
        }
        Ok(Self(values))
    }

    fn number(&self, option: &str) -> Option<u64> {
        match self.0.get(option) {
            Some(Value::Number(number)) => Some(*number),
            _ => None,
        }
    }

    fn path(&self, option: &str) -> Option<&'a Path> {
        match self.0.get(option) {
            Some(Value::Path(path)) => Some(path),
            _ => None,
        }
    }

    fn flag(&self, option: &str) -> bool {
        matches!(self.0.get(option), Some(Value::Flag))
    }

    fn stress_test(&self) -> Result<Option<stress::Test>, String> {
        use stress::Test::{Cpu, Network, Ram, Storage};

        let test = match self.0.get("--test") {
            Some(Value::StressTest(test)) => *test,
            _ => return Ok(None),
        };
        for (option, permitted) in [
            ("--duration-ms", matches!(test, Cpu | Ram)),
            ("--buffer-kib", test == Ram),
            ("--network-mib", test == Network),
            ("--storage-dir", test == Storage),
            ("--storage-mib", test == Storage),
            ("--storage-sync", test == Storage),
        ] {
            if !permitted && self.0.contains_key(option) {
                return Err(format!(
                    "'{option}' cannot be used with '--test {}'",
                    test.name()
                ));
            }
        }
        Ok(Some(test))
    }

    fn duration(&self, default_ms: u64) -> Duration {
        Duration::from_millis(self.number("--duration-ms").unwrap_or(default_ms))
    }

    fn timeout(&self, default_ms: u64) -> Duration {
        Duration::from_millis(self.number("--timeout-ms").unwrap_or(default_ms))
    }
}

fn help() {
    println!(
        "Usage:
  embedded-linux-benchmark cpu [--duration-ms MS]
  embedded-linux-benchmark ram [--duration-ms MS] [--buffer-kib KIB]
  embedded-linux-benchmark storage --dir DIR --size-mib MIB [--sync] [--timeout-ms MS]
  embedded-linux-benchmark network [--size-mib MIB] [--timeout-ms MS]
  embedded-linux-benchmark uart --device PATH --loopback [--bytes N] [--timeout-ms MS]
  embedded-linux-benchmark gpio --pin N [--samples N] [--timeout-ms MS]
  embedded-linux-benchmark stress auto [--test cpu|ram|network|storage] [--rounds N | --duration-secs SEC]
    [--duration-ms MS] [--buffer-kib KIB]
    [--network-mib MIB] [--timeout-ms MS] [--storage-dir DIR --storage-mib MIB [--storage-sync]]

cpu       Single-threaded 32-bit integer throughput
ram       Copy throughput between two equal-sized buffers
storage   Sequential buffered file I/O in the selected directory (1–64 MiB)
network   TCP throughput on 127.0.0.1 (1–64 MiB, default 8)
uart      Verified serial transfer at 115200 8N1 using a dedicated loopback fixture
gpio      Userspace read latency for an existing sysfs GPIO input
stress    Up to 5 sequential rounds of CPU, RAM, and TCP loopback (default 3)
          --duration-secs sets a total run target of 1–{max_duration_secs} seconds
          --duration-ms controls each CPU/RAM phase, including timed runs
          --test runs one component per round and accepts only its options
          Storage requires both a directory and a total write budget

cpu/ram --duration-ms  1–{MAX_DURATION_MS} (default {DEFAULT_DURATION_MS})
ram --buffer-kib       4–32768 per buffer (default 4096)
-h, --help  Show this help",
        max_duration_secs = stress::MAX_DURATION_SECS,
    );
}

fn help_requested(args: &[String]) -> bool {
    matches!(args, [flag] if flag == "--help" || flag == "-h")
}

fn parse_stress(args: &[String]) -> Result<stress::Plan, String> {
    let options = Options::parse("stress auto", args, STRESS_OPTIONS)?;
    let test = options.stress_test()?;
    let duration = options.duration(stress::DEFAULT_DURATION_MS);
    let schedule = match options.number("--duration-secs") {
        Some(seconds) => {
            for option in ["--rounds", "--timeout-ms"] {
                if options.number(option).is_some() {
                    return Err(format!("'{option}' cannot be used with '--duration-secs'"));
                }
            }
            stress::Schedule::Timed(Duration::from_secs(seconds))
        }
        None => {
            let count = options.number("--rounds").unwrap_or(stress::DEFAULT_ROUNDS);
            let timeout = options.timeout(stress::DEFAULT_TIMEOUT_MS);
            let timed_phases = match test {
                None => 2,
                Some(stress::Test::Cpu | stress::Test::Ram) => 1,
                Some(_) => 0,
            };
            if timeout <= duration * (timed_phases * count as u32) {
                let phases = if test.is_none() {
                    "combined CPU and RAM phase durations"
                } else {
                    "selected test's total phase duration"
                };
                return Err(format!("'--timeout-ms' must exceed the {phases}"));
            }
            stress::Schedule::Rounds { count, timeout }
        }
    };
    let sync = options.flag("--storage-sync");
    let storage = match (
        options.path("--storage-dir"),
        options.number("--storage-mib"),
    ) {
        (Some(path), Some(mib)) => Some(stress::Storage {
            directory: storage::directory(path)?,
            total_bytes: mib * MIB,
            sync,
        }),
        (None, None) if !sync && test != Some(stress::Test::Storage) => None,
        _ => return Err("stress storage requires both '--storage-dir' and '--storage-mib'".into()),
    };
    Ok(stress::Plan {
        test,
        schedule,
        duration,
        buffer_kib: options
            .number("--buffer-kib")
            .unwrap_or(stress::DEFAULT_BUFFER_KIB),
        network_bytes: options
            .number("--network-mib")
            .unwrap_or(network::DEFAULT_MIB)
            * MIB,
        storage,
    })
}

pub(crate) fn run() -> Result<(), String> {
    let args: Vec<String> = env::args_os()
        .skip(1)
        .map(|arg| arg.into_string().map_err(|_| "arguments must be UTF-8"))
        .collect::<Result<_, _>>()?;
    if args.is_empty() || help_requested(&args) {
        help();
        return Ok(());
    }
    let (command, args) = args.split_first().expect("command is present");
    let spec = match command.as_str() {
        "cpu" => CPU_OPTIONS,
        "ram" => RAM_OPTIONS,
        "storage" => STORAGE_OPTIONS,
        "network" => NETWORK_OPTIONS,
        "uart" => UART_OPTIONS,
        "gpio" => GPIO_OPTIONS,
        "stress" => {
            if help_requested(args) {
                help();
                return Ok(());
            }
            return match args.split_first() {
                Some((auto, args)) if auto == "auto" && help_requested(args) => {
                    help();
                    Ok(())
                }
                Some((auto, args)) if auto == "auto" => stress::run(parse_stress(args)?),
                _ => Err("expected 'stress auto'. See --help.".into()),
            };
        }
        _ => return Err(format!("unknown command '{command}'. See --help.")),
    };
    if help_requested(args) {
        help();
        return Ok(());
    }
    let options = Options::parse(command, args, spec)?;
    match command.as_str() {
        "cpu" => cpu::run(options.duration(DEFAULT_DURATION_MS), None),
        "ram" => ram::run(
            options.duration(DEFAULT_DURATION_MS),
            options
                .number("--buffer-kib")
                .unwrap_or(ram::DEFAULT_BUFFER_KIB),
            None,
        ),
        "storage" => {
            let directory = options.path("--dir").ok_or("storage requires '--dir'")?;
            let mib = options
                .number("--size-mib")
                .ok_or("storage requires '--size-mib'")?;
            let directory = storage::directory(directory)?;
            storage::run(
                &directory,
                mib * MIB,
                options.flag("--sync"),
                Deadline::after(options.timeout(storage::DEFAULT_TIMEOUT_MS)),
            )
        }
        "network" => network::run(
            options.number("--size-mib").unwrap_or(network::DEFAULT_MIB) * MIB,
            Deadline::after(options.timeout(network::DEFAULT_TIMEOUT_MS)),
        ),
        "uart" => {
            let device = options.path("--device").ok_or("UART requires '--device'")?;
            if !options.flag("--loopback") {
                return Err("UART requires '--loopback' to acknowledge a dedicated fixture".into());
            }
            uart::run(
                device,
                options.number("--bytes").unwrap_or(uart::DEFAULT_BYTES) as usize,
                Deadline::after(options.timeout(uart::DEFAULT_TIMEOUT_MS)),
            )
        }
        "gpio" => gpio::run(
            options.number("--pin").ok_or("GPIO requires '--pin'")? as u32,
            options.number("--samples").unwrap_or(gpio::DEFAULT_SAMPLES),
            Deadline::after(options.timeout(gpio::DEFAULT_TIMEOUT_MS)),
        ),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_day_and_two_week_targets_without_starting_work() {
        for seconds in [1, 86_400, 1_209_600] {
            let args = ["--duration-secs".into(), seconds.to_string()];
            let plan = parse_stress(&args).expect("valid stress plan");
            assert!(
                matches!(plan.schedule, stress::Schedule::Timed(duration) if duration == Duration::from_secs(seconds))
            );
        }
    }
}

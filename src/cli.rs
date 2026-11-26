use crate::common::{Deadline, DEFAULT_DURATION_MS, MAX_DURATION_MS, MIB};
use crate::report::{identify, Subject};
use crate::{
    camera, can, cpu, gpio, gpio_chardev, hdmi, i2c, iio, network, network_peer, pcie, pcie_health,
    ram, spi, storage, stress, uart, usb,
};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Kind {
    Number(u64, u64),
    Path,
    Text,
    Flag,
    StressTest,
    StressDuration,
    I2cAddress,
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
const NVME_HEALTH_OPTIONS: &[(&str, Kind)] = &[("--controller", Kind::Text)];
const STORAGE_OPTIONS: &[(&str, Kind)] = &[
    ("--dir", Kind::Path),
    ("--size-mib", Number(1, storage::MAX_MIB)),
    ("--timeout-ms", Number(1, storage::MAX_TIMEOUT_MS)),
    ("--sync", Flag),
];
const NETWORK_HEALTH_OPTIONS: &[(&str, Kind)] = &[
    ("--interface", Kind::Text),
    (
        "--sample-ms",
        Number(1, crate::network_health::MAX_SAMPLE_MS),
    ),
];
const NETWORK_OPTIONS: &[(&str, Kind)] = &[
    ("--size-mib", Number(1, network::MAX_MIB)),
    ("--timeout-ms", Number(1, network::MAX_TIMEOUT_MS)),
];
const PEER_OPTIONS: &[(&str, Kind)] = &[
    ("--bind", Kind::Text),
    ("--peer", Kind::Text),
    ("--session", Kind::Text),
    ("--port", Number(0, u16::MAX as u64)),
    ("--size-mib", Number(1, network_peer::MAX_MIB)),
    ("--pings", Number(1, network_peer::MAX_PINGS)),
    ("--timeout-ms", Number(1, network_peer::MAX_TIMEOUT_MS)),
];
const CAN_OPTIONS: &[(&str, Kind)] = &[
    ("--interface", Kind::Text),
    ("--tx-id", Kind::Text),
    ("--rx-id", Kind::Text),
    ("--session", Kind::Text),
    ("--fixture", Flag),
    ("--fd", Flag),
    ("--frames", Number(1, can::MAX_FRAMES)),
    ("--timeout-ms", Number(1, can::MAX_TIMEOUT_MS)),
];
const CAMERA_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Path),
    ("--frames", Number(2, camera::MAX_FRAMES)),
    ("--timeout-ms", Number(1, camera::MAX_TIMEOUT_MS)),
];
const HDMI_CAPTURE_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Path),
    ("--fixture", Flag),
    ("--frames", Number(2, camera::MAX_FRAMES)),
    ("--timeout-ms", Number(1, camera::MAX_TIMEOUT_MS)),
];
const HDMI_STATUS_OPTIONS: &[(&str, Kind)] = &[("--connector", Kind::Text)];
const UART_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Path),
    ("--loopback", Flag),
    ("--bytes", Number(1, uart::MAX_BYTES)),
    ("--timeout-ms", Number(1, uart::MAX_TIMEOUT_MS)),
];
const USB_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Path),
    ("--interface", Number(0, 255)),
    ("--in-endpoint", Kind::Text),
    ("--out-endpoint", Kind::Text),
    ("--loopback", Flag),
    ("--bytes", Number(1, usb::MAX_BYTES)),
    ("--timeout-ms", Number(1, usb::MAX_TIMEOUT_MS)),
];
const IIO_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Text),
    ("--channel", Kind::Text),
    ("--samples", Number(2, iio::MAX_SAMPLES)),
    ("--timeout-ms", Number(1, iio::MAX_TIMEOUT_MS)),
];
const INVENTORY_OPTIONS: &[(&str, Kind)] = &[
    ("--require-pcie", Kind::Text),
    ("--board-profile", Kind::Path),
    ("--thermal-sample-ms", Number(1, 60_000)),
    ("--evidence-json", Kind::Path),
];
const PCIE_LINK_OPTIONS: &[(&str, Kind)] = &[("--device", Kind::Text)];
const PCIE_HEALTH_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Text),
    ("--sample-ms", Number(1, pcie_health::MAX_SAMPLE_MS)),
    ("--min-width", Number(1, 32)),
    ("--min-speed-gtps", Kind::Text),
];
const PCIE_THROUGHPUT_OPTIONS: &[(&str, Kind)] = &[("--device", Kind::Text), ("--fixture", Flag)];
const I2C_OPTIONS: &[(&str, Kind)] = &[
    ("--adapter", Kind::Path),
    ("--address", Kind::I2cAddress),
    ("--fixture", Flag),
    ("--read-bytes", Number(1, i2c::MAX_READ_BYTES)),
    ("--samples", Number(1, i2c::MAX_SAMPLES)),
    ("--timeout-ms", Number(1, i2c::MAX_TIMEOUT_MS)),
];
const SPI_OPTIONS: &[(&str, Kind)] = &[
    ("--device", Kind::Path),
    ("--loopback", Flag),
    ("--bytes", Number(1, spi::MAX_BYTES)),
    ("--timeout-ms", Number(1, spi::MAX_TIMEOUT_MS)),
];
const GPIO_OPTIONS: &[(&str, Kind)] = &[
    ("--pin", Number(0, u32::MAX as u64)),
    ("--chip", Kind::Path),
    ("--offset", Number(0, u32::MAX as u64)),
    ("--input-fixture", Flag),
    ("--samples", Number(1, gpio::MAX_SAMPLES)),
    ("--timeout-ms", Number(1, gpio::MAX_TIMEOUT_MS)),
];
const QUALIFICATION_PASSIVE_OPTIONS: &[(&str, Kind)] = &[
    ("--board-profile", Kind::Path),
    ("--network-interface", Kind::Text),
    ("--nvme-controller", Kind::Text),
];
const QUALIFICATION_PLAN_OPTIONS: &[(&str, Kind)] =
    &[("--profile", Kind::Text), ("--include-local-load", Flag)];
const STRESS_OPTIONS: &[(&str, Kind)] = &[
    ("--test", Kind::StressTest),
    ("--rounds", Number(1, stress::MAX_ROUNDS)),
    ("--duration-secs", Number(1, stress::MAX_DURATION_SECS)),
    ("--duration", Kind::StressDuration),
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
    ("--summary-json", Kind::Path),
    ("--events-ndjson", Kind::Path),
];

enum Value<'a> {
    Number(u64),
    Path(&'a Path),
    Text(&'a str),
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
                Kind::Text => Value::Text(raw),
                Kind::I2cAddress => Value::Number(u64::from(i2c::address(raw)?)),
                Kind::StressDuration => Value::Number(stress::parse_duration(raw)?),
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

    fn text(&self, option: &str) -> Result<&'a str, String> {
        match self.0.get(option) {
            Some(Value::Text(value)) => Ok(value),
            _ => Err(format!("command requires '{option}'")),
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
            ("--duration-ms", matches!(test, Cpu | Ram | Network)),
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
  eld-engine accelerator inventory|interfaces
  eld-engine accelerator amdgpu-hwmon --device BDF
  eld-engine power ina3221 --node BUS-ADDRESS
  eld-engine inventory [--require-pcie BDF[=VVVV:DDDD][,...] | --board-profile PATH] [--thermal-sample-ms MS] [--evidence-json NEW_PATH]
  eld-engine qualify passive [--board-profile PATH] [--network-interface NAME] [--nvme-controller NAME]
  eld-engine qualify plan --profile bring-up|quick|manufacturing|overnight [--include-local-load]
  eld-engine cpu [--duration-ms MS]
  eld-engine ram [--duration-ms MS] [--buffer-kib KIB]
  eld-engine storage nvme-health --controller nvme0
  eld-engine storage nvme-smart-decode < captured-smart-log.bin
  eld-engine storage --dir DIR --size-mib MIB [--sync] [--timeout-ms MS]
  eld-engine network health --interface IFACE [--sample-ms MS]
  eld-engine network [--size-mib MIB] [--timeout-ms MS]
  eld-engine network peer-server|peer-client --bind IP --peer IP --port N
    --session HEX [--size-mib MIB] [--pings N] [--timeout-ms MS]
  eld-engine uart --device PATH --loopback [--bytes N] [--timeout-ms MS]
  eld-engine i2c --adapter PATH --address ADDR --fixture
    [--read-bytes N] [--samples N] [--timeout-ms MS]
  eld-engine spi --device PATH --loopback [--bytes N] [--timeout-ms MS]
  eld-engine gpio --pin N [--samples N] [--timeout-ms MS]
  eld-engine gpio --chip PATH --offset N --input-fixture
    [--samples N] [--timeout-ms MS]
  eld-engine camera --device PATH [--frames N] [--timeout-ms MS]
  eld-engine pcie link --device BDF
  eld-engine pcie health --device BDF [--sample-ms MS]
                        [--min-width LANES] [--min-speed-gtps GT_PER_SECOND]
  eld-engine pcie throughput --device BDF --fixture
  eld-engine iio --device NAME --channel INPUT [--samples N] [--timeout-ms MS]
  eld-engine usb --device PATH --interface N --out-endpoint ADDR --in-endpoint ADDR
    --loopback [--bytes N] [--timeout-ms MS]
  eld-engine can send|receive --interface IFACE --tx-id ID --rx-id ID
    --session HEX --fixture [--fd] [--frames N] [--timeout-ms MS]
  eld-engine hdmi status --connector NAME
  eld-engine hdmi capture --device PATH --fixture [--frames N] [--timeout-ms MS]
  eld-engine stress auto [--test cpu|ram|network|storage] [--rounds N | --duration TIME | --duration-secs SEC]
    [--duration-ms MS] [--buffer-kib KIB]
    [--network-mib MIB] [--timeout-ms MS] [--storage-dir DIR --storage-mib MIB [--storage-sync]]

cpu       Single-threaded 32-bit integer throughput
ram       Copy throughput between two equal-sized buffers
storage   Sequential buffered file I/O in the selected directory (1–64 MiB)
network   TCP throughput on 127.0.0.1 (1–64 MiB, default 8)
          peer-server/peer-client measure TCP throughput and application round trips
uart      Verified serial transfer at 115200 8N1 using a dedicated loopback fixture
i2c       Pure read latency using a dedicated fixture and one I2C address
spi       Verified full-duplex SPI throughput using a dedicated loopback fixture
gpio      Userspace input read latency via legacy sysfs or GPIO chardev v2
camera    V4L2 delivery rate, inter-arrival jitter, and driver sequence gaps
pcie      PCIe link speed and width observations. Stock endpoint throughput is unavailable.
iio       Userspace cadence and jitter for raw IIO input samples
usb       Verified bulk throughput on a dedicated usbfs loopback fixture
can       Classic CAN or CAN FD round trips with an isolated SocketCAN peer
hdmi      DRM connector observations or V4L2 capture-fixture delivery measurements
stress    Up to 5 rounds of concurrent CPU, RAM, and TCP loopback (default 3)
          Read-only PCIe, USB and HDMI observations; no device I/O without a separate request
          --duration accepts 30m, 12h, 14d or 2w (whole s/m/h/d/w); --duration-secs accepts 1–{max_duration_secs}
          --duration-ms sets the minimum CPU/RAM/loopback phase time ({default_phase_ms} ms, max {max_phase_ms})
          --test runs one component per round and accepts only its options
          Storage requires both a directory and a total write budget
          --summary-json NEW_PATH saves one private machine-readable run summary
          --events-ndjson NEW_PATH records start, sampled progress and finish

cpu/ram --duration-ms  1–{MAX_DURATION_MS} (default {DEFAULT_DURATION_MS})
ram --buffer-kib       4–32768 per buffer (default 4096)
-h, --help  Show this help",
        max_duration_secs = stress::MAX_DURATION_SECS,
        default_phase_ms = stress::DEFAULT_DURATION_MS,
        max_phase_ms = stress::MAX_DURATION_MS,
    );
}

fn help_requested(args: &[String]) -> bool {
    matches!(args, [flag] if flag == "--help" || flag == "-h")
}

fn parse_stress(args: &[String]) -> Result<stress::Plan, String> {
    let options = Options::parse("stress auto", args, STRESS_OPTIONS)?;
    let test = options.stress_test()?;
    let duration = options.duration(stress::DEFAULT_DURATION_MS);
    let seconds = options.number("--duration-secs");
    let human_duration = options.number("--duration");
    if seconds.is_some() && human_duration.is_some() {
        return Err("'--duration' cannot be used with '--duration-secs'".into());
    }
    let schedule = match seconds.or(human_duration) {
        Some(seconds) => {
            let timed_option = if human_duration.is_some() {
                "--duration"
            } else {
                "--duration-secs"
            };
            for option in ["--rounds", "--timeout-ms"] {
                if options.number(option).is_some() {
                    return Err(format!("'{option}' cannot be used with '{timed_option}'"));
                }
            }
            stress::Schedule::Timed(Duration::from_secs(seconds))
        }
        None => {
            let count = options.number("--rounds").unwrap_or(stress::DEFAULT_ROUNDS);
            let timeout = options.timeout(stress::DEFAULT_TIMEOUT_MS);
            if test != Some(stress::Test::Storage) && timeout <= duration * count as u32 {
                return Err("'--timeout-ms' must exceed the total scheduled load duration".into());
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
        summary_path: options.path("--summary-json").map(Path::to_path_buf),
        events_path: options.path("--events-ndjson").map(Path::to_path_buf),
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
    let (spec, subject) = match command.as_str() {
        "accelerator" => {
            match args {
                [mode] if mode == "inventory" => crate::accelerator::run(),
                [mode] if mode == "interfaces" => crate::accelerator_interfaces::run(),
                [mode, rest @ ..] if mode == "amdgpu-hwmon" => {
                    let options = Options::parse("accelerator amdgpu-hwmon", rest, PCIE_LINK_OPTIONS)?;
                    crate::accelerator_telemetry::run(options.text("--device")?)?;
                }
                _ => {
                    return Err(
                        "expected an accelerator subcommand: inventory, interfaces, or amdgpu-hwmon. See --help."
                            .into(),
                    )
                }
            }
            return Ok(());
        }
        "power" if args.first().map_or(false, |name| name == "ina3221") => {
            let options = Options::parse("power ina3221", &args[1..], &[("--node", Kind::Text)])?;
            crate::ina3221::run(options.text("--node")?)?;
            return Ok(());
        }
        "inventory" => {
            let options = Options::parse("inventory", args, INVENTORY_OPTIONS)?;
            crate::inventory::run(
                options.text("--require-pcie").ok(),
                options.path("--board-profile"),
                options.number("--thermal-sample-ms"),
                options.path("--evidence-json"),
            )?;
            return Ok(());
        }
        "qualify" if args.first().map_or(false, |role| role == "plan") => {
            let options = Options::parse("qualify plan", &args[1..], QUALIFICATION_PLAN_OPTIONS)?;
            println!(
                "{}",
                crate::qualification::render(
                    options.text("--profile")?,
                    options.flag("--include-local-load")
                )?
            );
            return Ok(());
        }
        "qualify" if args.first().map_or(false, |role| role == "passive") => {
            let options =
                Options::parse("qualify passive", &args[1..], QUALIFICATION_PASSIVE_OPTIONS)?;
            println!(
                "{}",
                crate::qualification::passive(
                    options.path("--board-profile"),
                    options.text("--network-interface").ok(),
                    options.text("--nvme-controller").ok()
                )?
            );
            return Ok(());
        }
        "qualify" => return Err("expected 'qualify plan' or 'qualify passive'. See --help.".into()),
        "cpu" => (CPU_OPTIONS, Subject::Cpu),
        "ram" => (RAM_OPTIONS, Subject::Ram),
        "storage" if args.first().map_or(false, |role| role == "nvme-health") => {
            let options = Options::parse("storage nvme-health", &args[1..], NVME_HEALTH_OPTIONS)?;
            return crate::nvme_health::run(options.text("--controller")?);
        }
        "storage"
            if args
                .first()
                .map_or(false, |role| role == "nvme-smart-decode") =>
        {
            Options::parse("storage nvme-smart-decode", &args[1..], &[])?;
            return crate::nvme_health::decode_stdin();
        }
        "storage" => (STORAGE_OPTIONS, Subject::Storage),
        "network" => match args.split_first() {
            Some((role, args)) if role == "health" => {
                let options = Options::parse("network health", args, NETWORK_HEALTH_OPTIONS)?;
                return crate::network_health::run(
                    options.text("--interface")?,
                    options.number("--sample-ms"),
                );
            }

            Some((role, args)) if matches!(role.as_str(), "peer-server" | "peer-client") => {
                if help_requested(args) {
                    help();
                    return Ok(());
                }
                return run_peer(role == "peer-server", args);
            }
            _ => (NETWORK_OPTIONS, Subject::NetworkLoopback),
        },
        "uart" => (UART_OPTIONS, Subject::Uart),
        "usb" => (USB_OPTIONS, Subject::Usb),
        "can" => {
            if help_requested(args) {
                help();
                return Ok(());
            }
            return run_can(args);
        }
        "pcie" => {
            if help_requested(args) {
                help();
                return Ok(());
            }
            return run_pcie(args);
        }
        "iio" => (IIO_OPTIONS, Subject::Iio),
        "camera" => (CAMERA_OPTIONS, Subject::Camera),
        "hdmi" => {
            if help_requested(args) {
                help();
                return Ok(());
            }
            return run_hdmi(args);
        }
        "i2c" => (I2C_OPTIONS, Subject::I2c),
        "spi" => (SPI_OPTIONS, Subject::Spi),
        "gpio" => (GPIO_OPTIONS, Subject::GpioSysfs),
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
    identify(subject);
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
        "i2c" => {
            let adapter = options
                .path("--adapter")
                .ok_or("I2C requires '--adapter'")?;
            let address = options
                .number("--address")
                .ok_or("I2C requires '--address'")? as u8;
            if !options.flag("--fixture") {
                return Err(
                    "I2C requires '--fixture' to acknowledge a dedicated read fixture".into(),
                );
            }
            i2c::run(
                adapter,
                address,
                options.number("--read-bytes").unwrap_or(1) as usize,
                options.number("--samples").unwrap_or(i2c::DEFAULT_SAMPLES),
                Deadline::after(options.timeout(i2c::DEFAULT_TIMEOUT_MS)),
            )
        }
        "spi" => {
            let device = options.path("--device").ok_or("SPI requires '--device'")?;
            if !options.flag("--loopback") {
                return Err("SPI requires '--loopback' to acknowledge a dedicated fixture".into());
            }
            spi::run(
                device,
                options.number("--bytes").unwrap_or(spi::DEFAULT_BYTES) as usize,
                Deadline::after(options.timeout(spi::DEFAULT_TIMEOUT_MS)),
            )
        }
        "gpio" => run_gpio(options),
        "camera" => run_camera(options, false),
        "iio" => iio::run(
            options.text("--device")?,
            options.text("--channel")?,
            options.number("--samples").unwrap_or(iio::DEFAULT_SAMPLES),
            Deadline::after(options.timeout(iio::DEFAULT_TIMEOUT_MS)),
        ),

        "usb" => {
            if !options.flag("--loopback") {
                return Err("USB requires '--loopback' to acknowledge a dedicated fixture".into());
            }
            let config = usb::Config {
                device: options.path("--device").ok_or("USB requires '--device'")?,
                interface: options
                    .number("--interface")
                    .ok_or("USB requires '--interface'")? as u32,
                output: usb::endpoint(options.text("--out-endpoint")?, false)?,
                input: usb::endpoint(options.text("--in-endpoint")?, true)?,
                bytes: options.number("--bytes").unwrap_or(usb::DEFAULT_BYTES) as usize,
            };
            usb::run(
                &config,
                Deadline::after(options.timeout(usb::DEFAULT_TIMEOUT_MS)),
            )
        }

        _ => unreachable!(),
    }
}

fn run_pcie(args: &[String]) -> Result<(), String> {
    let (command, args) = args
        .split_first()
        .ok_or("expected 'pcie link', 'pcie health', or 'pcie throughput'")?;
    if help_requested(args) {
        help();
        return Ok(());
    }
    match command.as_str() {
        "link" => {
            let options = Options::parse("pcie link", args, PCIE_LINK_OPTIONS)?;
            identify(Subject::PcieLink);
            pcie::link(options.text("--device")?)
        }
        "health" => {
            let options = Options::parse("pcie health", args, PCIE_HEALTH_OPTIONS)?;
            let expectation = pcie_health::LinkExpectation::parse(
                options.number("--min-width"),
                options.text("--min-speed-gtps").ok(),
            )?;
            pcie_health::run(
                options.text("--device")?,
                options.number("--sample-ms"),
                expectation,
            )
        }
        "throughput" => {
            let options = Options::parse("pcie throughput", args, PCIE_THROUGHPUT_OPTIONS)?;
            identify(Subject::PcieThroughput);
            pcie::throughput(options.text("--device")?, options.flag("--fixture"))
        }
        _ => Err("expected 'pcie link', 'pcie health', or 'pcie throughput'".into()),
    }
}

fn run_can(args: &[String]) -> Result<(), String> {
    let (role, args) = args
        .split_first()
        .ok_or("expected 'can send' or 'can receive'")?;
    let sender = match role.as_str() {
        "send" => true,
        "receive" => false,
        _ => return Err("expected 'can send' or 'can receive'".into()),
    };
    if help_requested(args) {
        help();
        return Ok(());
    }
    let options = Options::parse("can", args, CAN_OPTIONS)?;
    if !options.flag("--fixture") {
        return Err("CAN requires '--fixture' to acknowledge a dedicated isolated bus".into());
    }
    let config = can::Config {
        interface: can::interface(options.text("--interface")?)?,
        tx: can::identifier(options.text("--tx-id")?)?,
        rx: can::identifier(options.text("--rx-id")?)?,
        session: can::session(options.text("--session")?)?,
        frames: options.number("--frames").unwrap_or(128) as u32,
        fd: options.flag("--fd"),
    };
    if config.tx == config.rx {
        return Err("CAN transmit and receive IDs must be distinct".into());
    }
    identify(if sender {
        Subject::CanSend
    } else {
        Subject::CanReceive
    });
    can::run(&config, sender, Deadline::after(options.timeout(10_000)))
}

fn run_hdmi(args: &[String]) -> Result<(), String> {
    let (command, args) = args
        .split_first()
        .ok_or("expected 'hdmi status' or 'hdmi capture'")?;
    if help_requested(args) {
        help();
        return Ok(());
    }
    match command.as_str() {
        "status" => {
            let options = Options::parse("hdmi status", args, HDMI_STATUS_OPTIONS)?;
            identify(Subject::HdmiStatus);
            hdmi::status(options.text("--connector")?)
        }
        "capture" => run_camera(
            Options::parse("hdmi capture", args, HDMI_CAPTURE_OPTIONS)?,
            true,
        ),
        _ => Err("expected 'hdmi status' or 'hdmi capture'".into()),
    }
}

fn run_camera(options: Options<'_>, hdmi: bool) -> Result<(), String> {
    let device = options
        .path("--device")
        .ok_or("capture requires '--device'")?;
    if hdmi && !options.flag("--fixture") {
        return Err(
            "HDMI capture requires '--fixture' to acknowledge a dedicated capture fixture".into(),
        );
    }
    if hdmi {
        identify(Subject::HdmiCapture);
    }
    camera::run(
        device,
        options.number("--frames").unwrap_or(camera::DEFAULT_FRAMES),
        Deadline::after(options.timeout(camera::DEFAULT_TIMEOUT_MS)),
        hdmi,
    )
}

fn run_peer(server: bool, args: &[String]) -> Result<(), String> {
    let options = Options::parse("network peer", args, PEER_OPTIONS)?;
    let bind = network_peer::address(options.text("--bind")?)?;
    let peer = network_peer::address(options.text("--peer")?)?;
    if bind == peer || bind.is_loopback() != peer.is_loopback() {
        return Err(
            "'--bind' and '--peer' must be distinct and both loopback or both non-loopback".into(),
        );
    }
    let port = options
        .number("--port")
        .ok_or("network peer requires '--port'")? as u16;
    if !server && port == 0 {
        return Err("peer client requires a nonzero '--port'".into());
    }
    let config = network_peer::Config {
        bind,
        peer,
        port,
        session: network_peer::session(options.text("--session")?)?,
        mib: options.number("--size-mib").unwrap_or(8),
        pings: options.number("--pings").unwrap_or(20),
    };
    let deadline = Deadline::after(options.timeout(network_peer::MAX_TIMEOUT_MS));
    if server {
        identify(Subject::NetworkPeerServer);
        network_peer::server(&config, deadline)
    } else {
        identify(Subject::NetworkPeerClient);
        network_peer::client(&config, deadline)
    }
}

fn run_gpio(options: Options<'_>) -> Result<(), String> {
    let pin = options.number("--pin");
    let chip = options.path("--chip");
    let offset = options.number("--offset");
    let fixture = options.flag("--input-fixture");
    let samples = options.number("--samples").unwrap_or(gpio::DEFAULT_SAMPLES);
    let deadline = Deadline::after(options.timeout(gpio::DEFAULT_TIMEOUT_MS));
    if chip.is_some() || offset.is_some() || fixture {
        if pin.is_some() {
            return Err("'--pin' cannot be combined with GPIO chardev options".into());
        }
        let chip = chip.ok_or("GPIO chardev requires '--chip'")?;
        let offset = offset.ok_or("GPIO chardev requires '--offset'")? as u32;
        if !fixture {
            return Err(
                "GPIO chardev requires '--input-fixture' to acknowledge a spare input".into(),
            );
        }
        identify(Subject::GpioCharacterDevice);
        return gpio_chardev::run(chip, offset, samples, deadline);
    }
    gpio::run(
        pin.ok_or("GPIO requires '--pin' or GPIO chardev options")? as u32,
        samples,
        deadline,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn qualification_load_round_trips_through_the_actual_stress_parser() {
        use crate::qualification::planned_load;
        for (profile, expected_secs, expected_kib) in [
            ("quick", 1, 1024),
            ("manufacturing", 30, 4096),
            ("overnight", 14400, 8192),
        ] {
            assert!(planned_load(profile, false).unwrap().is_empty());
            let args = planned_load(profile, true).unwrap();
            assert_eq!(args.len(), 2);
            for (name, seconds, argv) in args {
                assert_eq!(&argv[..2], ["stress", "auto"]);
                let parsed = parse_stress(&argv[2..]).unwrap();
                let selected = parsed
                    .test
                    .expect("default mixed stress is forbidden in qualification");
                assert_eq!(selected.name(), name);
                assert_eq!(seconds, expected_secs);
                assert!(matches!(parsed.schedule, stress::Schedule::Timed(duration)
                    if duration == Duration::from_secs(expected_secs)));
                assert!(parsed.storage.is_none());
                if name == "ram" {
                    assert_eq!(parsed.buffer_kib, expected_kib);
                }
                let without_test =
                    parse_stress(&argv[4..].iter().take(2).cloned().collect::<Vec<_>>()).unwrap();
                assert!(
                    without_test.test.is_none(),
                    "parser default would broaden the workload"
                );
            }
        }
        assert!(planned_load("bring-up", false).unwrap().is_empty());
        assert!(planned_load("bring-up", true).is_err());
    }

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

    #[test]
    fn parses_sustained_defaults_and_maximum_fixed_round_plan() {
        let default = parse_stress(&[]).expect("default stress plan");
        assert_eq!(default.duration, Duration::from_secs(10));
        assert_eq!(default.buffer_kib, 1024);
        assert!(matches!(
            default.schedule,
            stress::Schedule::Rounds { count: 3, timeout } if timeout == Duration::from_secs(60)
        ));

        let max = [
            "--rounds",
            "5",
            "--duration-ms",
            "60000",
            "--timeout-ms",
            "360000",
        ]
        .map(String::from);
        let plan = parse_stress(&max).expect("maximum duration and round count");
        assert_eq!(plan.duration, Duration::from_secs(60));
        assert!(matches!(
            plan.schedule,
            stress::Schedule::Rounds { count: 5, timeout } if timeout == Duration::from_secs(360)
        ));
    }

    #[test]
    fn parses_friendly_durations_without_starting_work() {
        for (input, seconds) in [
            ("1s", 1),
            ("30m", 1800),
            ("12h", 43200),
            ("14d", 1209600),
            ("2w", 1209600),
            ("1209600", 1209600),
        ] {
            let args = ["--duration".into(), input.into()];
            let plan = parse_stress(&args).expect("valid stress duration");
            assert!(
                matches!(plan.schedule, stress::Schedule::Timed(duration) if duration == Duration::from_secs(seconds))
            );
        }
        for input in [
            "0",
            "0h",
            "15d",
            "3w",
            "1.5h",
            "99999999999999999999w",
            "2weeks",
            "",
            "2週",
        ] {
            let args = ["--duration".into(), input.into()];
            assert!(parse_stress(&args).is_err(), "{input}");
        }
    }
}

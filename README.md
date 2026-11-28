<div align="center">

# Embedded Linux Diagnostic Engine

[![CI](https://github.com/brucerry/embedded-linux-diagnostic-engine/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/brucerry/embedded-linux-diagnostic-engine/actions/workflows/ci.yml?query=branch%3Amain)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Hardware benchmarks and diagnostics for Linux boards and servers, in a dependency-free Rust CLI.

</div>

---

## Run on a board

Pick the verified [release binary](https://github.com/brucerry/embedded-linux-diagnostic-engine/releases) that matches the **board's CPU and C library**, or build it with the board's SDK. Match its SHA-256 against the release checksums before copying it. On the board, name the executable `eld-engine`:

```sh
chmod +x ./eld-engine
./eld-engine --help
./eld-engine capabilities
./eld-engine inventory
```

The examples below use `eld-engine` on PATH. Use `./eld-engine` if you copied it into the current directory. Replace example IDs and paths with those on your board. A command can be built in even when the board lacks the device, driver, kernel interface, or permission it needs.

---

## On-board diagnostic commands

### Passive inspection

These commands do not generate test traffic or write to the selected device.

| Component | Built-in command | What it checks |
| --- | --- | --- |
| [GPU or accelerator](docs/gpu.md) | `eld-engine accelerator inventory`<br>`eld-engine accelerator interfaces`<br>`eld-engine pcie health --device 0000:65:00.0 --sample-ms 1000` | Enumerated device, driver, registered interfaces, PCIe link, and available error counters; not a compute test |
| [AMD GPU sensors](docs/gpu.md#amd-gpu-sensors) | `eld-engine accelerator amdgpu-hwmon --device 0000:65:00.0` | Temperature and supported power channels on the selected AMDGPU-bound device |
| [Jetson power rails](docs/power.md#selected-ina3221-power-rail-readings) | `eld-engine power ina3221 --node 1-0040` | Voltage and current on the selected rail monitor, not necessarily GPU-only |
| [Board, CPU, memory, thermal](docs/system.md#read-host-resource-inventory) | `eld-engine inventory` | Board and kernel metadata, resources, and available thermal and error counters |
| [hwmon temperature](docs/system.md#read-one-hwmon-temperature-channel) | `eld-engine hwmon temperature --sensor hwmon0 --channel 1` | One selected raw temperature-channel reading, without assuming units |
| [CPU, memory, I/O pressure](docs/system.md#passive-linux-pressure-snapshot) | `eld-engine pressure` | Linux PSI where available |
| [Expected PCIe device](docs/pcie.md#check-expected-pci-devices) | `eld-engine inventory --require-pcie 0000:01:00.0=14c3:7990` | Whether the expected vendor/device is at that PCI address |
| [PCIe locality](docs/pcie.md#selected-pcie-function-locality) | `eld-engine pcie locality --device 0000:01:00.0` | Kernel-reported NUMA node and local CPU hints for one function |
| [Ethernet or Wi-Fi interface](docs/network.md#passive-network-link-health) | `eld-engine network health --interface eth0 --sample-ms 1000` | Link state and comparable error/drop counter changes, not throughput |
| [RDMA port](docs/network.md#passive-local-rdma-port) | `eld-engine fabric rdma-port --device mlx5_0 --port 1` | Selected local port state and kernel-reported rate, not measured throughput |
| [NVMe controller](docs/storage.md#passive-nvme-controller-status) | `eld-engine storage nvme-health --controller nvme0` | Controller state, transport, model, and firmware; not a live SMART read |
| [eMMC](docs/storage.md#passive-mmc-wear-observations) | `eld-engine storage mmc-health --card mmc0:0001` | Cached wear indicators when the driver exposes them; not SD card support |
| [HDMI output](docs/display.md#hdmi-connector-observations-and-capture-delivery) | `eld-engine hdmi status --connector card0-HDMI-A-1` | Connection and advertised modes, not active signal timing |

### Active tests on a lab board

Use dedicated fixtures for bus tests. Don't run them against an in-use device, console, or production link. See the [peripheral fixture guide](docs/peripherals.md) for wiring, prerequisites, and limits.

| Component | Built-in command | Prerequisite |
| --- | --- | --- |
| [CPU / RAM](docs/system.md#cpu-and-ram) | `eld-engine cpu --duration-ms 500` / `eld-engine ram --duration-ms 500 --buffer-kib 1024` | Brief local load |
| [Storage](docs/storage.md#storage) | `eld-engine storage --dir /tmp --size-mib 4` | Writable scratch space with 4 MiB free; use a mounted test volume to measure storage rather than RAM-backed `/tmp` |
| [TCP](docs/network.md#tcp-loopback) | `eld-engine network --size-mib 4` | Local loopback only; [peer mode](docs/network.md#tcp-peer-throughput-and-round-trip-latency) for a separate lab link |
| [UART](docs/peripherals.md#uart-loopback) | `eld-engine uart --device /dev/ttyUSB0 --loopback` | Dedicated serial loopback, not a console |
| [I²C](docs/peripherals.md#i2c-fixture-read-latency) | `eld-engine i2c --adapter /dev/i2c-1 --address 0x48 --fixture --read-bytes 2` | Isolated fixture that supports those reads |
| [SPI](docs/peripherals.md#spi-fixture-throughput) | `eld-engine spi --device /dev/spidev0.0 --loopback` | Dedicated MOSI↔MISO loopback |
| [GPIO input](docs/peripherals.md#gpio-input-read-latency-chardev-v2) | `eld-engine gpio --chip /dev/gpiochip0 --offset 7 --input-fixture` | Spare input using the GPIO v2 ABI |
| [IIO input](docs/peripherals.md#iio-raw-input-cadence) | `eld-engine iio --device iio:device0 --channel in_voltage0 --samples 100` | Known sensor channel safe for repeated reads |
| [USB](docs/peripherals.md#usb-bulk-loopback) | `eld-engine usb --device /dev/bus/usb/001/011 --interface 0 --out-endpoint 0x01 --in-endpoint 0x81 --loopback` | Dedicated bulk echo fixture with matching endpoints |
| [CAN / CAN FD](docs/peripherals.md#classic-can-and-can-fd-peer-fixture) | `eld-engine can receive --interface can0 --tx-id 0x124 --rx-id 0x123 --session 1234abcd --fixture` | Isolated two-node bus; start the receiver, then use the [matching sender](docs/peripherals.md#classic-can-and-can-fd-peer-fixture) |
| [Camera](docs/display.md#camera-delivery-rate) | `eld-engine camera --device /dev/video0 --frames 60` | Selected V4L2 capture device |
| [HDMI input capture](docs/display.md#hdmi-connector-observations-and-capture-delivery) | `eld-engine hdmi capture --device /dev/video0 --fixture --frames 60` | Dedicated HDMI capture source on V4L2 |
| [Stress](docs/stress.md#automatic-stress) | `eld-engine stress auto --test cpu --duration 5s` | Short CPU-only load; the default suite also runs RAM and TCP loopback |

---

## Build for a target

Build natively on the board or select the Rust target, linker, and standard library that match its SDK and C library. The crate sets none of those for you.

| Environment | Verification |
| --- | --- |
| Linux x86-64 | Native Debian builds and CLI tests |
| OpenWrt | Selected AArch64 SDKs from 21.02 through a pinned upstream revision, plus x86-64, Armv7, and both MIPS byte orders. Package and guest evidence varies by target. |
| NVIDIA Jetson Linux | AArch64 vendor SDK builds for 36.5.2 and 39.2.2, with CLI checks in matching userspaces under QEMU user mode |

Platform coverage varies by benchmark. Physical-board performance requires a run on the board and firmware you are evaluating.

---

## Guides

- [Diagnostic guides by component](docs/benchmarks.md) indexes all topic guides.
- [Validation and hardware coverage](docs/validation.md) records what was tested and where.
- [OpenWrt AArch64](docs/openwrt-compatibility.md) and [other OpenWrt architectures](docs/openwrt-platforms.md) cover SDK selection, packages, and guest checks.
- [Jetson Linux](docs/jetson-linux.md) covers NVIDIA SDK cross-builds and ABI checks.
- [Optional CUDA helper](docs/cuda.md) covers separate NVIDIA Toolkit builds and compute and transfer modes.
- [Optional ALSA helper](docs/audio.md) covers dedicated PCM loopback and software round-trip estimates.
- [Optional Jetson H.264 helper](docs/jetson-codec.md) covers separate hardware encode and decode fixtures.

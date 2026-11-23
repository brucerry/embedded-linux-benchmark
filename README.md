<div align="center">

# Embedded Linux Benchmark

[![CI](https://github.com/brucerry/embedded-linux-benchmark/actions/workflows/ci.yml/badge.svg?branch=cpu-ram-benchmarks)](https://github.com/brucerry/embedded-linux-benchmark/actions/workflows/ci.yml?query=branch%3Acpu-ram-benchmarks)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

A dependency-free Rust CLI for measuring individual components of a Linux system or embedded board.

</div>

## Get started

Install Rust and Cargo 1.66 or newer and a native C linker on Linux.

```sh
cargo build --release --locked
./target/release/embedded-linux-benchmark --help
./target/release/embedded-linux-benchmark cpu
./target/release/embedded-linux-benchmark ram --duration-ms 500 --buffer-kib 1024
./target/release/embedded-linux-benchmark stress auto
```

The default stress run uses CPU, RAM, and TCP loopback. Storage writes require a directory and an explicit size budget. Peer networking and peripheral benchmarks require an explicitly selected peer, device, or fixture. See the [benchmark guide](docs/benchmarks.md) for commands, limits, and how to interpret results.

## Build for a target

Build natively on the board or select the Rust target, linker, and standard library that match its SDK and C library. The crate sets none of those for you.

| Environment | Verification |
| --- | --- |
| Linux x86-64 | Native Debian builds and CLI tests |
| OpenWrt | Selected AArch64 SDKs from 21.02 through a pinned upstream revision, plus x86-64, Armv7, and both MIPS byte orders. Package and guest evidence varies by target. |
| NVIDIA Jetson Linux | AArch64 vendor SDK builds for 36.5.2 and 39.2.2, with CLI checks in matching userspaces under QEMU user mode |

Platform coverage varies by benchmark. Physical-board performance requires a run on the board and firmware you are evaluating.

## Guides

- [Benchmarks and fixtures](docs/benchmarks.md) covers CPU, RAM, storage, networking, stress, and peripheral tests.
- [OpenWrt AArch64](docs/openwrt-compatibility.md) and [other OpenWrt architectures](docs/openwrt-platforms.md) cover SDK selection, packages, and guest checks.
- [Jetson Linux](docs/jetson-linux.md) covers NVIDIA SDK cross-builds and ABI checks.
- [Optional CUDA helper](docs/cuda.md) covers separate NVIDIA Toolkit builds and compute and transfer modes.
- [Optional ALSA helper](docs/audio.md) covers dedicated PCM loopback and software round-trip estimates.
- [Optional Jetson H.264 helper](docs/jetson-codec.md) covers separate hardware encode and decode fixtures.

# embedded-linux-benchmark

A dependency-free Rust CLI for measuring CPU, RAM, filesystem, and TCP loopback throughput on Linux. It also runs a bounded automatic stress suite and provides opt-in UART loopback and GPIO input benchmarks for embedded boards.

## Build and run

On a Linux distribution or a Jetson running Jetson Linux, build natively with Rust and Cargo 1.66 or newer and a native C linker. The crate has no external Rust dependencies.

```sh
cargo build --release --locked
cargo test --release --locked
./target/release/embedded-linux-benchmark --help
./target/release/embedded-linux-benchmark cpu
./target/release/embedded-linux-benchmark ram --duration-ms 500 --buffer-kib 1024
./target/release/embedded-linux-benchmark network --size-mib 8
./target/release/embedded-linux-benchmark stress auto
```

See the [benchmark guide](docs/benchmarks.md) for storage opt-in, the stress limits, and measurement details. The [peripheral guide](docs/benchmarks.md#peripherals) covers the UART fixture and the GPIO input requirements.

For other machines running a different Linux release, build against a C library baseline they provide. For NVIDIA Jetson cross-builds, use the [Jetson Linux SDK build and ABI checks](docs/jetson-linux.md).

For a cross-build, use the Rust target and standard library that match your firmware's architecture and ABI. With your SDK environment loaded, select the target and, when required, its linker through Cargo. For example, for a 64-bit Arm musl board:

```sh
rustup target add aarch64-unknown-linux-musl
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=/path/to/sdk/bin/target-cc \
  cargo build --release --locked --target aarch64-unknown-linux-musl
```

Use your SDK's actual linker path and any flags or sysroot it requires. The crate sets no target, linker, or C library. Copy the resulting binary from `target/<target>/release/` to the matching board and run it there.

For installable OpenWrt packages, use the [SDK overlay and AArch64 matrix](docs/openwrt-compatibility.md) or the [x86-64, Armv7, and MIPS build guide](docs/openwrt-platforms.md). The SDK selects IPK or APK for its release.

## Reading the results

- **CPU** reports iterations per second for a fixed 32-bit xorshift loop, with a checksum of the final state. Each iteration applies three shift-and-XOR updates to the same integer.
- **RAM** reports MiB/s for copying one preallocated buffer into another. It prints the size of each buffer and the combined working set. Each copy counts the source size once. Cache residency and the copy routine affect the result, so interpret it as application-level copy throughput.
- Both use one thread and monotonic elapsed time. `--duration-ms` defaults to 1000 and accepts 1–60000. Elapsed time can exceed the requested duration by a batch or copy and any scheduling delay. The RAM benchmark accepts 4–32768 KiB per buffer, defaults to 4096 KiB per buffer, and uses up to 64 MiB for both buffers. It initializes the buffers before timing and checks the final contents after timing.

## Source layout

`src/main.rs` handles exit status and `src/cli.rs` validates and dispatches commands. The `cpu`, `ram`, `storage`, `network`, `uart`, and `gpio` modules implement the workloads. `src/stress.rs` runs the automatic suite, and `src/common.rs` contains shared timing and deadline logic.

## Validation

The [current suite results](docs/benchmarks.md#validation) cover the new commands. The SDK references below also retain the earlier CPU and RAM evidence.

| Target | Evidence |
| --- | --- |
| x86_64 Linux GNU | Native release build and CLI checks on a Debian development host with Rust 1.66.0 |
| Jetson Linux 39.2.2 and 36.5.2 on AArch64 | [NVIDIA SDK cross-builds and QEMU user-mode CLI checks](docs/jetson-linux.md) |
| OpenWrt 21.02 through pinned upstream `main` on AArch64 | [Per-release SDK builds and QEMU guest installation](docs/openwrt-compatibility.md) |
| OpenWrt x86-64, Armv7 hard-float, and MIPS32 in both byte orders | [Selected SDK builds and QEMU guest installation](docs/openwrt-platforms.md) |
| OpenWrt 25.12.6 `armsr/armv8` with the release-pinned Rust helper | [Separate SDK APK and QEMU guest proof](docs/openwrt-package.md), plus a [standalone binary smoke test](docs/openwrt-25.12.6.md) |
| Actual embedded Linux boards | No board runs recorded |

A native build checks the development host. An emulated smoke test checks whether the target binary starts and produces valid output under emulation. Board performance requires a run on the actual board and firmware.

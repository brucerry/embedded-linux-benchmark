# NVIDIA Jetson Linux builds

Use the [native Linux build](../README.md#build-and-run) on a Jetson with a suitable Rust and C toolchain. To build on an x86-64 Linux host for a Jetson, target `aarch64-unknown-linux-gnu` and use the GCC driver designated for the Jetson Linux release on the target. It supplies the linker and its development sysroot. The benchmark uses the Rust standard library for CPU and RAM work.

## Choose the release

NVIDIA lists JetPack 7.2.2 with Jetson Linux 39.2.2 as the current release as of November 23, 2026, with Ubuntu 24.04 and support for Jetson Thor T5000 and T4000 and the Jetson Orin family. JetPack 6.2.3 with Jetson Linux 36.5.2 is the older Orin release tested here, based on Ubuntu 22.04. [JetPack 7.2.2 download and release information](https://developer.nvidia.com/embedded/jetpack/downloads) · [Jetson Linux 36.5.2 release information](https://developer.nvidia.com/embedded/jetson-linux-r3652)

| Jetson Linux | NVIDIA-designated toolchain | GCC driver relative to the extraction directory | Toolchain glibc |
| --- | --- | --- | --- |
| 39.2.2 | Crosstool-NG GCC 13.2.0 | `x-tools/aarch64-none-linux-gnu/bin/aarch64-none-linux-gnu-gcc` | 2.28 |
| 36.5.2 | Bootlin GCC 11.3.0, `2022.08-1` | `aarch64--glibc--stable-2022.08-1/bin/aarch64-buildroot-linux-gnu-gcc` | 2.35 |

Download the archive linked by the guide for your release. NVIDIA's 39.2.2 guide points to the `x-tools.tbz2` also used for the 38.2 release. These SHA-256 values pin the archives used in verification. [39.2.2 toolchain guide](https://docs.nvidia.com/jetson/archives/r39.2.2/DeveloperGuide/AT/JetsonLinuxToolchain.html) · [36.5.2 toolchain guide](https://docs.nvidia.com/jetson/archives/r36.5.2/DeveloperGuide/AT/JetsonLinuxToolchain.html)

| Archive | SHA-256 |
| --- | --- |
| `x-tools.tbz2` | `6bf10ad05bc6a5f296e592a78d87771b87f0dc917337ba9f4ab4b0f7aabad889` |
| `aarch64--glibc--stable-2022.08-1.tar.bz2` | `8af54f268c462b2d0737df8789b5e35db03a2d1ecbec90e20948f66f9244fcdd` |

## Cross-build and inspect the binary

Verify the selected archive with `sha256sum`, extract it, and run the following from the benchmark checkout. Set `sdk_gcc` to the absolute path to the GCC driver from the table. Keep the GCC filename for the Bootlin wrapper. Choose a separate output directory when switching releases.

```sh
rustup toolchain install 1.96.0 --profile minimal --target aarch64-unknown-linux-gnu
sdk_gcc=/path/to/extracted/toolchain/bin/aarch64-none-linux-gnu-gcc
out="$PWD/target/jetson-r39.2.2"
"$sdk_gcc" -dumpmachine
"$sdk_gcc" -print-sysroot

CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$sdk_gcc" \
CARGO_TARGET_DIR="$out" \
  cargo +1.96.0 build --release --locked --offline --target aarch64-unknown-linux-gnu

binary="$out/aarch64-unknown-linux-gnu/release/eld-engine"
readelf -W -h -l -d -V "$binary"
```

With a separate development sysroot from your SDK, point Cargo at a GCC wrapper that forwards the arguments and adds `--sysroot=/path/to/sysroot`. The development sysroot needs the target CRT objects and link libraries. Use the matching root filesystem to test runtime compatibility. The installed loader and libraries must provide the interpreter path, shared library names, and versioned symbols reported by `readelf`.

## Verify CLI behavior

Use QEMU user mode on the build host with the SDK sysroot or a matching extracted AArch64 root filesystem as the loader prefix. This runs the benchmark process using the selected userspace libraries and the host kernel.

```sh
runtime=$("$sdk_gcc" -print-sysroot)
qemu-aarch64 -L "$runtime" "$binary" --help
qemu-aarch64 -L "$runtime" "$binary" cpu --duration-ms 4
qemu-aarch64 -L "$runtime" "$binary" ram --duration-ms 4 --buffer-kib 4
status=0
qemu-aarch64 -L "$runtime" "$binary" cpu --duration-ms 0 || status=$?
test "$status" -eq 1
status=0
qemu-aarch64 -L "$runtime" "$binary" ram --buffer-kib 32769 || status=$?
test "$status" -eq 1
```

Each invalid request should report its accepted range. For the 4 KiB RAM example, expect an 8 KiB working set and 4096 bytes per reported copy.

On a physical Jetson, copy the executable to a user-writable directory and run the same CLI commands directly. Record the board model, Jetson Linux release, and runtime glibc when collecting performance results.

## Evidence

Both cross-builds used Rust 1.96.0 and the designated NVIDIA toolchain on x86-64 Debian 13. The resulting ELFs were AArch64 with interpreter `/lib/ld-linux-aarch64.so.1`. Both require `libc.so.6` and `libgcc_s.so.1`. The 39.2.2 build also lists `libpthread.so.0` and `libdl.so.2`.

| Build | Highest required glibc symbol | CLI test environment | Result |
| --- | --- | --- | --- |
| x86-64 native build with Rust 1.66.0 | `GLIBC_2.34` | Debian 13, glibc 2.41 | Native release tests and CLI smoke passed |
| Jetson Linux 39.2.2 cross-build | `GLIBC_2.28` | QEMU user mode with the GCC sysroot and with Ubuntu 24.04.5 sample libraries, glibc `2.39-0ubuntu8.9` | Both passed |
| Jetson Linux 36.5.2 cross-build | `GLIBC_2.34` | QEMU user mode with the GCC sysroot and with Ubuntu 22.04.5 sample libraries, glibc `2.35-0ubuntu3.13` | Both passed |

The sample-filesystem tests used QEMU 10.0.13 with the AArch64 loader and shared libraries extracted from NVIDIA's release archives. Both complete archives matched NVIDIA's published SHA-1 hashes. The tests checked help, the CPU checksum, RAM buffer sizes and byte accounting, and invalid duration and buffer size. Physical Jetson boards, JetPack boot and integration, and GPU or CUDA execution remain untested. [39.2.2 release hashes](https://developer.nvidia.com/downloads/embedded/L4T/r39_Release_v2.2/release/release_sha_hashes.txt) · [36.5.2 release hashes](https://developer.nvidia.com/downloads/embedded/l4t/r36_release_v5.2/releases/release_sha_hashes.txt)

| NVIDIA sample filesystem | Verified vendor SHA-1 | Recorded SHA-256 |
| --- | --- | --- |
| 39.2.2 AArch64 | `39211d66a088782b9e3e3a77fc885d25b189ca2e` | `2a84d949eb3bbab14900bcc4171c74aa080734038872d69e67667d5c1587dd07` |
| 36.5.2 AArch64 | `fb95b63ddf297a5f42613c94b2085eaa23e740f2` | `077da9b27b1d3bc2247c2b73ce10cce880bdefe1f048d2a64df878d855a5b435` |

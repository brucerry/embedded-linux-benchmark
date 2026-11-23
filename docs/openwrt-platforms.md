# OpenWrt x86-64, Armv7, and MIPS

The [SDK overlay](../openwrt/Makefile) packages the same locked Rust crate across the target combinations below. The SDK supplies the native linker, sysroot, package architecture, and package format. The [AArch64 matrix](openwrt-compatibility.md) covers the existing 21.02 through upstream builds.

## Verified SDK and guest pairs

| OpenWrt SDK and target | Rust target and ABI | Rust compile | Linked ELF interpreter | SDK package | Matching QEMU guest |
| --- | --- | --- | --- | --- | --- |
| [21.02.7 `x86/64`](https://downloads.openwrt.org/releases/21.02.7/targets/x86/64/) | `x86_64-unknown-linux-musl` | Pass | `/lib/ld-musl-x86_64.so.1` | IPK `x86_64` | Pass |
| [22.03.7 `armvirt/32`](https://downloads.openwrt.org/releases/22.03.7/targets/armvirt/32/) | `armv7-unknown-linux-musleabihf`, EABI5 hard-float | Pass | `/lib/ld-musl-armhf.so.1` | IPK `arm_cortex-a15_neon-vfpv4` | Pass |
| [24.10.8 `malta/be`](https://downloads.openwrt.org/releases/24.10.8/targets/malta/be/) | `mips-unknown-linux-musl`, MIPS32r2 o32 soft-float, big endian | Pass | `/lib/ld-musl-mips-sf.so.1` | IPK `mips_24kc` | Pass |
| [24.10.8 `malta/le`](https://downloads.openwrt.org/releases/24.10.8/targets/malta/le/) | `mipsel-unknown-linux-musl`, MIPS32r2 o32 soft-float, little endian | Pass | `/lib/ld-musl-mipsel-sf.so.1` | IPK `mipsel_24kc` | Pass |
| [25.12.6 `armsr/armv7`](https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv7/) | `armv7-unknown-linux-musleabihf`, EABI5 hard-float | Pass | `/lib/ld-musl-armhf.so.1` | APK `arm_cortex-a15_neon-vfpv4` | Pass |
| [Snapshot `x86/64`](https://downloads.openwrt.org/snapshots/targets/x86/64/) | `x86_64-unknown-linux-musl` | Pass | `/lib/ld-musl-x86_64.so.1` | APK `x86_64` | Pass |

The x86-64 snapshot is `r38523-14a07b78e9`. It matches the upstream commit checked on November 23, 2026. Each packaged ELF declares `libc.so` and `libgcc_s.so.1`. The six QEMU 10.0.13 guests booted the corresponding checksum-verified OpenWrt kernel and root filesystem with no network device. In each guest the package manager installed the package, and checks passed for help, the CPU checksum, the RAM working set and byte count, invalid duration and buffer size, and removal. Physical boards remain untested. [Pinned upstream commit](https://github.com/openwrt/openwrt/commit/14a07b78e9ac9d604547533550e6aa41955ac588)

### SDK archive hashes

Verify each archive against the published `sha256sums` in its target directory. For the moving snapshot URL, match both the SHA-256 and `version.buildinfo` before reproducing that row.

| SDK archive | SHA-256 |
| --- | --- |
| `openwrt-sdk-21.02.7-x86-64_gcc-8.4.0_musl.Linux-x86_64.tar.xz` | `21c75e4c5a429da487f8a885555cb49eee80cfc355d5dbe35d2b3ae05324a09d` |
| `openwrt-sdk-22.03.7-armvirt-32_gcc-11.2.0_musl_eabi.Linux-x86_64.tar.xz` | `69f6345618413c17178132ffcb7bd6cb3cc8a62c2ea1df30e6640fa23076ffe4` |
| `openwrt-sdk-24.10.8-malta-be_gcc-13.3.0_musl.Linux-x86_64.tar.zst` | `759cbd8879c44e14bcc59ef8dd71d758dca2e106797c252cc21ac0ea42fa0cd0` |
| `openwrt-sdk-24.10.8-malta-le_gcc-13.3.0_musl.Linux-x86_64.tar.zst` | `3f2c57511619cdb9aa22b7445f52095b4dc4b787a1eeebb03eedc1cbd13f4ed1` |
| `openwrt-sdk-25.12.6-armsr-armv7_gcc-14.3.0_musl_eabi.Linux-x86_64.tar.zst` | `b29c5b96d757c3518a4f4df18e3187e16de618381625847b8874461935358179` |
| `openwrt-sdk-x86-64_gcc-15.3.0_musl.Linux-x86_64.tar.zst` | `669207b98dd1298195ca5e8996db8c2c032348f7032cb5592d70283babed5807` |

## Build with the matching Rust toolchain

Follow the [overlay staging steps](openwrt-compatibility.md#build-a-package) in the selected SDK. For x86-64 and Armv7, use Rust 1.96.0 and set `target` to the triple in the table. Check the SDK libc's ELF attributes with `readelf -h -A` before choosing an Arm target. Both tested Arm SDKs have the hard-float calling convention, even though their archive names end in `_musl_eabi`. The soft-float `armv7-unknown-linux-musleabi` std failed both linker checks with VFP register ABI errors.

For MIPS, use the row's endian-specific target. Rust 1.96.0 provides no downloadable std for either of these Tier 3 targets. The verified route uses the official nightly compiler with `rust-src` and builds std for the selected target. Fetch the Rust library's locked dependencies into the same Cargo cache that the SDK build uses. The package build runs offline. [Rust MIPS target support](https://doc.rust-lang.org/rustc/platform-support.html) · [Cargo `build-std`](https://doc.rust-lang.org/cargo/reference/unstable.html#build-std)

```sh
nightly=nightly-2026-05-27
target=mips-unknown-linux-musl  # Use mipsel-unknown-linux-musl for malta/le.
rustup toolchain install "$nightly" --profile minimal --component rust-src
rust_src="$(rustc +"$nightly" --print sysroot)/lib/rustlib/src/rust"
cargo +"$nightly" fetch --locked --manifest-path "$rust_src/library/Cargo.toml"
rust_bin=$(dirname "$(rustup which --toolchain "$nightly" cargo)")

# Run in the SDK after staging the overlay and running make defconfig.
PATH="$rust_bin:$PATH" make package/embedded-linux-benchmark/compile \
  OPENWRT_RUST_TARGET="$target" OPENWRT_BUILD_STD=1 V=s
```

`OPENWRT_BUILD_STD=1` selects Cargo's `-Z build-std`. The default `0` uses the selected toolchain's installed std. Run `make package/embedded-linux-benchmark/clean` when switching compiler, target, or std build mode in the same SDK.

## Repeat the guest smoke test

Use the kernel and root filesystem from the table's matching target directory. The x86-64 tests used QEMU `pc` with `qemu64`. The Armv7 tests used `virt` with `cortex-a15`. Both booted an uncompressed `newc` initramfs containing the OpenWrt root filesystem and the test package, following the [disposable guest procedure](openwrt-package.md#test-installation-in-a-disposable-guest). The MIPS tests used `qemu-system-mips` for big endian and `qemu-system-mipsel` for little endian, with `-machine malta -cpu 24Kc`. Stage the same files in a writable ext4 image using `mke2fs -d`, attach it with `-drive file=guest.ext4,format=raw,if=ide`, and boot with `root=/dev/sda rootfstype=ext4 rw rootwait init=/package-smoke`.

Install the local IPK with `opkg install` or install the APK from the SDK-signed local index. Create `/var/lock` first if running the IPK test script as PID 1. Check `--help`, `cpu --duration-ms 4`, and `ram --duration-ms 4 --buffer-kib 4`. The RAM working set should be 8 KiB and bytes copied should equal copies times 4096. The commands `cpu --duration-ms 0` and `ram --buffer-kib 32769` should exit with status 1 and a range error. Remove the package and check that `/usr/bin/embedded-linux-benchmark` was removed.

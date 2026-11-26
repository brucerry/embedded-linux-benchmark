# OpenWrt SDK compatibility

The opt-in SDK overlay builds the locked Cargo crate with an external Rust compiler and target standard library. It uses the selected OpenWrt SDK's GCC linker driver and sysroot, then lets the SDK produce its native package format. This works with the 21.02.7 and 22.03.7 release-pinned packages feeds, which lack `lang/rust/rust-package.mk`. [21.02.7 feed pins](https://downloads.openwrt.org/releases/21.02.7/targets/armvirt/64/feeds.buildinfo) · [22.03.7 feed pins](https://downloads.openwrt.org/releases/22.03.7/targets/armvirt/64/feeds.buildinfo)

## Verified AArch64 SDK builds

These are the published stable series from 21.02 through 25.12 and a published snapshot matching the official upstream `main` head on November 23, 2026. The target names vary by release. All six rows used an x86_64 Linux build host and upstream Rust/Cargo 1.96.0 with `aarch64-unknown-linux-musl` std. [OpenWrt release index](https://downloads.openwrt.org/releases/) · [Upstream commit `14a07b78e9ac9d604547533550e6aa41955ac588`](https://github.com/openwrt/openwrt/commit/14a07b78e9ac9d604547533550e6aa41955ac588)

| SDK release and target | Build revision | Linked ELF | Package | Full-system QEMU smoke | Physical board |
| --- | --- | --- | --- | --- | --- |
| [21.02.7 `armvirt/64`](https://downloads.openwrt.org/releases/21.02.7/targets/armvirt/64/) | `r16847-f8282da11e` | AArch64 musl | IPK, `aarch64_cortex-a53` | Install, CLI, removal passed | Unverified |
| [22.03.7 `armvirt/64`](https://downloads.openwrt.org/releases/22.03.7/targets/armvirt/64/) | `r20341-591b7e93d3` | AArch64 musl | IPK, `aarch64_cortex-a53` | Install, CLI, removal passed | Unverified |
| [23.05.6 `armsr/armv8`](https://downloads.openwrt.org/releases/23.05.6/targets/armsr/armv8/) | `r24232-539228933c` | AArch64 musl | IPK, `aarch64_generic` | Install, CLI, removal passed | Unverified |
| [24.10.8 `armsr/armv8`](https://downloads.openwrt.org/releases/24.10.8/targets/armsr/armv8/) | `r29233-443ec4032a` | AArch64 musl | IPK, `aarch64_generic` | Install, CLI, removal passed | Unverified |
| [25.12.6 `armsr/armv8`](https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv8/) | `r33311-981c978eff` | AArch64 musl | APK, `aarch64_generic` | Install, CLI, removal passed | Unverified |
| [Snapshot `armsr/armv8`](https://downloads.openwrt.org/snapshots/targets/armsr/armv8/) | `r38523-14a07b78e9` | AArch64 musl | APK, `aarch64_generic` | Install, CLI, removal passed | Unverified |

Each package contained `/usr/bin/eld-engine`. Inspection found `/lib/ld-musl-aarch64.so.1` as the interpreter and `libc.so` and `libgcc_s.so.1` as shared dependencies. The tests booted the checksum-verified matching OpenWrt kernels and root filesystems in QEMU 10.0.13 with no network device, installed the IPKs with `opkg` and the APKs from local repositories with signed indexes, and exercised help, the CPU checksum, the RAM working set and byte count, and invalid input. They also verified package removal. These checks cover the listed SDK and userspace pairs. Board and firmware measurements require physical runs.

### SDK archives and published SHA-256 values

Use the SDK directory linked in the matrix for the archive and `sha256sums`. For the moving snapshot URL, match both the recorded archive digest and `version.buildinfo` to reproduce this row.

| SDK archive | SHA-256 |
| --- | --- |
| `openwrt-sdk-21.02.7-armvirt-64_gcc-8.4.0_musl.Linux-x86_64.tar.xz` | `d4bec0392c0a49b2235fc83d077c3a7cd268dc716bc7b06c1e15eba6dee5be84` |
| `openwrt-sdk-22.03.7-armvirt-64_gcc-11.2.0_musl.Linux-x86_64.tar.xz` | `6dd26293ad4b89f66bee55d131f4937f82ff810134c6600e23210163098bc547` |
| `openwrt-sdk-23.05.6-armsr-armv8_gcc-12.3.0_musl.Linux-x86_64.tar.xz` | `0b8e27de3c4ec90a1155d0ee5f4a7622b2d90f4efced59b346b10ddfdb396fe6` |
| `openwrt-sdk-24.10.8-armsr-armv8_gcc-13.3.0_musl.Linux-x86_64.tar.zst` | `5f430f5b30c9ea6dc472710356c139abf916b7ebd5de14e108e9cc204f40a2a4` |
| `openwrt-sdk-25.12.6-armsr-armv8_gcc-14.3.0_musl.Linux-x86_64.tar.zst` | `cbf3343bc3d6dc6180fc84077b966c13596a79904adaadbfe68409d563b0b596` |
| `openwrt-sdk-armsr-armv8_gcc-15.3.0_musl.Linux-x86_64.tar.zst` | `22bc4d2748cdbc895aa145ee8319e256b7f3b4592134f466086779ef222a35a3` |

## Build a package

Start in the benchmark checkout with the selected SDK archive already downloaded and verified. Use the standard SDK build prerequisites. The older SDKs require a Python interpreter with `distutils` available. If the SDK selects a different interpreter, set `staging_dir/host/bin/python` and `python3` in the SDK to the working host interpreter and let the prerequisite check run. The example uses AArch64. See the [platform guide](openwrt-platforms.md) for the target triples and Rust toolchains for x86-64, Armv7, and MIPS.

```sh
checkout=$PWD
sdk=/path/to/extracted-openwrt-sdk
target=aarch64-unknown-linux-musl
rustup toolchain install 1.96.0 --profile minimal --target "$target"
rust_bin=$(dirname "$(rustup which --toolchain 1.96.0 cargo)")

umask 022
cd "$sdk"
overlay=package/eld-engine
install -D -m 0644 "$checkout/openwrt/Makefile" "$overlay/Makefile"
mkdir -p "$overlay/src"
cp "$checkout/Cargo.toml" "$checkout/Cargo.lock" "$checkout/LICENSE" "$overlay/src/"
cp -R "$checkout/src" "$overlay/src/"
printf 'CONFIG_PACKAGE_eld-engine=m\n' > .config
make defconfig
PATH="$rust_bin:$PATH" make package/eld-engine/compile \
  OPENWRT_RUST_TARGET="$target" V=s
find bin/packages -name 'eld-engine*.ipk' -o -name 'eld-engine*.apk'
```

The example creates `.config` in a fresh SDK directory. In a configured SDK, select `CONFIG_PACKAGE_eld-engine=m` in your existing configuration. Run the package `clean` target before rebuilding with a different external Rust toolchain or target.

Choose a Rust target and standard library matching the SDK's CPU ABI and libc. Rust 1.96.0 linked and ran on every row above. The crate's native minimum remains Rust 1.66, whose AArch64 musl std failed to link against SDKs 24.10.8 and 25.12.6 with undefined references to `stat64`, `open64`, and `fstat64`. Other architecture and firmware combinations require their own verification.

Install an IPK with `opkg install /path/to/package.ipk` in a matching OpenWrt environment. For APKs, the [25.12.6 guest procedure](openwrt-package.md#test-installation-in-a-disposable-guest) shows how to create a signed local index and verify installation and removal. Use the APK produced by the SDK in the selected row.

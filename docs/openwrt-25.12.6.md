# OpenWrt 25.12.6 `armsr/armv8` build and smoke test

The release binary was cross-built on x86_64 Debian 13 using GCC 14.3.0 and the musl sysroot from the official OpenWrt SDK. QEMU user-mode 10.0.13 ran it against libraries from the matching generic OpenWrt root filesystem (`r33311-981c978eff`). The ELF header identified AArch64 and requested `/lib/ld-musl-aarch64.so.1`. [OpenWrt release files](https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv8/)

This check used upstream Rust 1.96.0 and its AArch64 musl standard library through `rustup`, with the SDK's GCC as the linker. The [APK build and full-system guest check](openwrt-package.md) has its own recipe and evidence.

## Reproduce

Run these commands from the crate checkout on an x86_64 Linux host with `curl`, `tar` with zstd support, Rustup, and `qemu-aarch64` available.

```sh
proof=$(mktemp -d)
release=https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv8
sdk_name=openwrt-sdk-25.12.6-armsr-armv8_gcc-14.3.0_musl.Linux-x86_64
rootfs_name=openwrt-25.12.6-armsr-armv8-generic-targz-rootfs.tar.gz

curl -fsSL "$release/$sdk_name.tar.zst" -o "$proof/$sdk_name.tar.zst"
curl -fsSL "$release/$rootfs_name" -o "$proof/$rootfs_name"
curl -fsSL "$release/feeds.buildinfo" -o "$proof/feeds.buildinfo"
(
  cd "$proof"
  sha256sum -c <<EOF
cbf3343bc3d6dc6180fc84077b966c13596a79904adaadbfe68409d563b0b596  $sdk_name.tar.zst
b781548d30886845176fee8256e115188029de0c42af252ffc6ea94a62794381  $rootfs_name
037f68494e8cf059f7cb49dbce7e0f73a3da4db1e73b2b8b7477ff2d2676f40b  feeds.buildinfo
EOF
) || exit 1
tar -xf "$proof/$sdk_name.tar.zst" -C "$proof"
mkdir "$proof/rootfs"
tar -xf "$proof/$rootfs_name" -C "$proof/rootfs" --exclude='./dev/*'

rustup toolchain install 1.96.0 --profile minimal --target aarch64-unknown-linux-musl
sdk="$proof/$sdk_name"
toolchain="$sdk/staging_dir/toolchain-aarch64_generic_gcc-14.3.0_musl"
STAGING_DIR="$sdk/staging_dir/target-aarch64_generic_musl" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$toolchain/bin/aarch64-openwrt-linux-musl-gcc" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS='-C target-feature=-crt-static' \
CARGO_TARGET_DIR="$proof/target" \
  cargo +1.96.0 build --release --locked --offline --target aarch64-unknown-linux-musl

program="$proof/target/aarch64-unknown-linux-musl/release/embedded-linux-benchmark"
qemu-aarch64 -L "$proof/rootfs" "$program" --help
qemu-aarch64 -L "$proof/rootfs" "$program" cpu --duration-ms 20
qemu-aarch64 -L "$proof/rootfs" "$program" ram --duration-ms 20 --buffer-kib 4
qemu-aarch64 -L "$proof/rootfs" "$program" ram --duration-ms 20 --buffer-kib 1024
qemu-aarch64 -L "$proof/rootfs" "$program" ram --buffer-kib 32769
```

The smoke test verified a CPU checksum for the reported iteration count, RAM working sets of 8 and 2048 KiB, and byte and throughput calculations. The oversized RAM request exited with status 1 and printed the accepted range to stderr. Use emulator output to check CLI behavior. Board performance requires running the CLI on the actual board and firmware.

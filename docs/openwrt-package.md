# OpenWrt 25.12.6 `armsr/armv8` package

This records the separate `r1` build using the [recipe at `45e2867`](https://github.com/brucerry/embedded-linux-benchmark/blob/45e286739130fc59d606dff24d149e952a514df0/openwrt/Makefile) and the packages feed's Rust helper. It produced `embedded-linux-benchmark-0.1.0-r1.apk` for `aarch64_generic`, with `/usr/bin/embedded-linux-benchmark` and runtime dependencies on `libc` and `libgcc1`. The [current overlay](openwrt-compatibility.md) shares one package recipe across release SDKs.

## Build in the release SDK

Start at the crate checkout on an x86_64 Linux host with the OpenWrt SDK build prerequisites. This recipe uses GNU Make, GNU awk, rsync, and the ncurses development headers among the standard prerequisites. Use a fresh SDK directory for this example.

```sh
checkout=$PWD
work=$(mktemp -d)
release=https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv8
sdk_name=openwrt-sdk-25.12.6-armsr-armv8_gcc-14.3.0_musl.Linux-x86_64
umask 022

cd "$work"
curl -fsSLO "$release/$sdk_name.tar.zst"
curl -fsSLO "$release/feeds.buildinfo"
sha256sum -c <<EOF || exit 1
cbf3343bc3d6dc6180fc84077b966c13596a79904adaadbfe68409d563b0b596  $sdk_name.tar.zst
037f68494e8cf059f7cb49dbce7e0f73a3da4db1e73b2b8b7477ff2d2676f40b  feeds.buildinfo
EOF
tar -xf "$sdk_name.tar.zst"
sdk="$work/$sdk_name"
cd "$sdk"
./scripts/feeds update base packages
test "$(git -C feeds/base_root rev-parse HEAD)" = 64900e756efa667774214f9c6b17c113222e01f7 || exit 1
test "$(git -C feeds/packages rev-parse HEAD)" = 093d551675b1119a06ef224d419905e8d6b2af1a || exit 1
./scripts/feeds install -p packages rust

overlay=package/embedded-linux-benchmark
mkdir -p "$overlay/src"
git -C "$checkout" show 45e286739130fc59d606dff24d149e952a514df0:openwrt/Makefile > "$overlay/Makefile"
cp "$checkout/Cargo.toml" "$checkout/Cargo.lock" "$checkout/LICENSE" "$overlay/src/"
cp -R "$checkout/src" "$overlay/src/"
printf 'CONFIG_PACKAGE_embedded-linux-benchmark=m\n' > .config
make defconfig
```

The recipe declares `rust/host` as a build dependency. The standard SDK target `make package/embedded-linux-benchmark/compile V=s` builds that toolchain from the feed as needed. The tested path below puts upstream Rust 1.96.0, the version specified by the pinned feed, on `PATH` and skips the SDK host compiler build. The release-pinned `rust-package.mk` still drives Cargo and the SDK still links and packages the binary. [SDK and release files](https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv8/) · [Pinned Rust helper](https://github.com/openwrt/packages/blob/093d551675b1119a06ef224d419905e8d6b2af1a/lang/rust/rust-package.mk)

```sh
rustup toolchain install 1.96.0 --profile minimal --target aarch64-unknown-linux-musl
rust_bin=$(dirname "$(rustup which --toolchain 1.96.0 cargo)")
PATH="$rust_bin:$PATH" make OPENWRT_BUILD=1 \
  -o package/feeds/packages/rust/host/compile package/embedded-linux-benchmark/compile V=s

package="$sdk/bin/packages/aarch64_generic/base/embedded-linux-benchmark-0.1.0-r1.apk"
"$sdk/staging_dir/host/bin/apk" adbdump "$package"
```

## Test installation in a disposable guest

The smoke test used QEMU system emulation 10.0.13 with the release `generic-kernel.bin` and `generic-targz-rootfs.tar.gz`, verified against the published SHA-256 sums. It booted the release userspace as an uncompressed `newc` initramfs with a smoke-test init script and no network device. [OpenWrt kernel and root filesystem](https://downloads.openwrt.org/releases/25.12.6/targets/armsr/armv8/)

On the build host, stage the local APK and its signed repository index in a disposable copy of the release root filesystem. The guest uses the SDK-generated public key to verify the index.

```sh
rootfs_name=openwrt-25.12.6-armsr-armv8-generic-targz-rootfs.tar.gz
kernel_name=openwrt-25.12.6-armsr-armv8-generic-kernel.bin
curl -fsSL "$release/$rootfs_name" -o "$work/$rootfs_name"
curl -fsSL "$release/$kernel_name" -o "$work/$kernel_name"
(
  cd "$work"
  sha256sum -c <<EOF
b781548d30886845176fee8256e115188029de0c42af252ffc6ea94a62794381  $rootfs_name
2b9ce48df5536e993ee691339057828488e4a31be9c2bb5d3f6120f1135fcbb0  $kernel_name
EOF
) || exit 1
guest="$work/guest"
mkdir -p "$guest/package-repo" "$guest/package-keys"
tar -xf "$work/$rootfs_name" -C "$guest"
cp "$package" "$guest/package-repo/"
cp "$sdk/public-key.pem" "$guest/package-keys/benchmark.pem"
"$sdk/staging_dir/host/bin/apk" mkndx --allow-untrusted \
  --sign "$sdk/private-key.pem" --output "$guest/package-repo/packages.adb" "$package"
```

With `cpio` and `qemu-system-aarch64` on the host, boot the guest and run the install, CLI, and removal checks. The smoke test also checked package ownership, the CPU checksum, RAM byte accounting, and the oversized buffer error on stderr.

```sh
cat > "$guest/package-smoke" <<'EOF'
#!/bin/sh
set -eu
export PATH=/usr/sbin:/usr/bin:/sbin:/bin
trap 'result=$?; printf "\nPackage smoke exit: %s\n" "$result"; poweroff -f' EXIT
test -c /dev/null || mknod /dev/null c 1 3
mount -t proc proc /proc
mount -t sysfs sysfs /sys
. /etc/openwrt_release
test "$DISTRIB_RELEASE" = 25.12.6
test "$DISTRIB_TARGET" = armsr/armv8
apk --no-network --repositories-file /dev/null --keys-dir /package-keys \
  --repository file:///package-repo/packages.adb add embedded-linux-benchmark
apk --no-network --repositories-file /dev/null info --who-owns /usr/bin/embedded-linux-benchmark
embedded-linux-benchmark --help
embedded-linux-benchmark cpu --duration-ms 20
embedded-linux-benchmark ram --duration-ms 20 --buffer-kib 4
status=0
embedded-linux-benchmark ram --buffer-kib 32769 || status=$?
test "$status" -eq 1
apk --no-network --repositories-file /dev/null del embedded-linux-benchmark
test ! -e /usr/bin/embedded-linux-benchmark
EOF
chmod 0755 "$guest/package-smoke"
(
  cd "$guest"
  find . -print0 | cpio --null --quiet -o -H newc --owner=0:0
) > "$work/package-guest.cpio"
qemu-system-aarch64 -machine virt -cpu cortex-a57 -m 512 -nodefaults \
  -nographic -serial stdio -monitor none -no-reboot \
  -kernel "$work/$kernel_name" -initrd "$work/package-guest.cpio" \
  -append 'console=ttyAMA0 rdinit=/package-smoke panic=-1'
```

The package build and guest checks cover OpenWrt 25.12.6 `armsr/armv8` with the SDK linker, the pinned helper, and upstream Rust. A source build of the feed's Rust host toolchain and physical-board runs remain unverified.

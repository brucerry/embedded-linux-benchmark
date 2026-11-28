# Validation and hardware coverage

## Validation

Use the [hardware smoke checklist](hardware-smoke.md) to record physical results and fixture limits.

The native Debian 13 release tests cover component output, loopback byte counts, invalid limits and incomplete storage opt-ins, storage budget accounting, and scratch-file removal. A child process with a small file-size limit exercises a write failure both directly and in the stress suite. The default three-round run also passed without creating a scratch file.

All four individual selectors passed native CLI tests, an OpenWrt 25.12.6 AArch64 full-system QEMU test, and Jetson Linux 39.2.2 QEMU user-mode checks with the matching NVIDIA sample libraries. These runs checked the selected phase count, storage byte accounting and cleanup, and rejection of conflicting settings.

Short elapsed-time runs passed on native Debian 13, in matching OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS32 little-endian QEMU guests, and under QEMU user mode with Jetson Linux 39.2.2 sample libraries. They exercised deadline clamping, the shared storage budget, storage-only early exit, and scratch cleanup. The 14-day boundary and overflow were tested through argument parsing. The full duration was not run.

UART pseudoterminal tests passed natively and in the Jetson Linux 39.2.2 QEMU user-mode setup. They exercised loopback byte counts, corruption, timeout, terminal refusal, and tty restoration. The GPIO unit test uses ordinary files as inert attributes to check input-only reads, counts, direction and type rejection, and timeout.

```sh
cargo test --release --locked
cargo build --release --locked
python3 tests/uart_pty.py ./target/release/eld-engine
```

The peripheral packages were built and inspected with matching OpenWrt SDKs. Package installation, UART pseudoterminal transfer and failure cases, Linux console refusal, missing GPIO rejection, and package removal passed in the full-system QEMU guests below. GPIO availability records whether the kernel exposed `/sys/class/gpio/export`. The tests inspected its presence without opening it or exporting a pin.

| Environment | Kernel | Legacy GPIO sysfs export attribute |
| --- | --- | --- |
| OpenWrt 21.02.7 `armvirt/64` and `x86/64` (IPK) | 5.4.238 | Present in both guests |
| OpenWrt 24.10.8 `malta/le` and `malta/be` (IPK) | 6.6.144 | Present in both guests |
| OpenWrt 25.12.6 `armsr/armv7` and `armsr/armv8` (APK) | 6.12.112 | Present in both guests |
| Native Debian 13 development environment | Host kernel | Absent in the test environment |
| Jetson Linux 39.2.2 QEMU user mode | Host kernel | Jetson kernel availability unverified |

Physical UART rates, GPIO read latency, and GPIO availability on actual boards remain unverified.

## I2C, SPI, and GPIO chardev verification

Rust fixtures cover full and short I2C reads, SPI chunking and received-data verification, and GPIO direction, busy-line, and release behavior. CLI tests cover missing fixture acknowledgment, invalid addresses and limits, and rejection of unrelated device nodes. The C assertions check the request encodings and structures against native Linux and SDK headers, including both MIPS32 byte orders. Use the SDK C compiler in place of `cc` for a cross check. For Linux 5.4 headers, add `-DEXPECT_LEGACY_GPIO_HEADERS` to check the interfaces those headers expose.

```sh
cargo test --release --locked
cc -std=c11 -Wall -Wextra -Werror -c tests/linux_uapi.c -o /tmp/benchmark-uapi.o
```

| Environment | Verified in this change |
| --- | --- |
| OpenWrt 25.12.6 `armsr/armv7` and `armsr/armv8` | Matching APK build, install, invalid-input checks, and removal in QEMU. GPIO v2 read counts, busy refusal, offset validation, and line release passed on an emulated PL061 chip. |
| OpenWrt 21.02.7 `armvirt/64` | Matching IPK build and guest lifecycle passed. Linux 5.4 rejected GPIO v2 on the emulated PL061 and the CLI reported it as unavailable. |
| OpenWrt 24.10.8 `malta/le` and `malta/be` | Matching IPK builds and guest lifecycle passed, including invalid I2C, SPI, and GPIO inputs. Rust ABI and mock transaction tests passed in QEMU user mode in both byte orders. |
| Jetson Linux 39.2.2 AArch64 | SDK build and ABI checks passed. Mock transactions and CLI rejection passed under QEMU user mode with matching sample libraries. |

I2C and SPI transaction validation used mocks. Physical bus transfers, physical GPIO latency, and Jetson kernel peripheral availability remain unverified.

| Current build | Execution evidence |
| --- | --- |
| OpenWrt 21.02.7 `armvirt/64` AArch64 IPK | Matching full-system QEMU install, suite checks, removal |
| OpenWrt 25.12.6 `armsr/armv8` AArch64 APK | Matching full-system QEMU install, suite checks, removal |
| OpenWrt 24.10.8 `malta/le` MIPS32 IPK | Matching full-system QEMU install, suite checks, removal |
| Jetson Linux 36.5.2 and 39.2.2 AArch64 GNU/Linux | QEMU user mode with each SDK sysroot and matching NVIDIA sample libraries |

The OpenWrt guests enabled loopback in the test setup. The Jetson tests used the host kernel, filesystem, and TCP stack through QEMU user mode. The SDK checks exercised CPU and RAM, storage with sync and cleanup, TCP loopback, and stress with and without opted-in storage. Physical-board measurements remain unverified. [OpenWrt SDK records](openwrt-compatibility.md) · [MIPS SDK records](openwrt-platforms.md) · [Jetson SDK records](jetson-linux.md)

## Evaluate planned PCIe requirements offline

Use `scripts/evaluate_pcie_requirements.py --help` with the trusted preview,
the exact saved report and stdout, and the observed process exit status.
A positively identified device passes only the declared requirement.
Missing or mismatched devices fail; unavailable observations stay
unavailable. No requirements means not assessed. This score does not
certify the board or establish hardware, transport, or manufacturing
provenance. Read the JSON outcome rather than using tool exit as a pass.

For synthetic and saved management-plane observations, see [offline BMC evidence](bmc.md).
It is separate from physical board and firmware verification.

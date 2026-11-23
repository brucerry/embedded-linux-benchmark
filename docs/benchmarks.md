# Storage, network, peripherals, and automatic stress

Use `cargo build --release --locked` and run `./target/release/embedded-linux-benchmark --help` for the commands. All workloads run on the calling thread. The `cpu` and `ram` commands also run independently with their existing settings.

## Storage

Select an existing writable directory and a size of 1–64 MiB. The command creates a uniquely named regular file with owner-only permissions, writes the requested payload sequentially, then reads and verifies the same byte count. Exclusive creation protects existing files. The command removes the scratch file on success or a handled error and reports the path if removal fails.

```sh
./target/release/embedded-linux-benchmark storage --dir /path/to/writable-directory --size-mib 16
./target/release/embedded-linux-benchmark storage --dir /path/to/writable-directory --size-mib 16 --sync
```

The write and read rates are separate. Writes go through buffered filesystem I/O. With `--sync`, write timing includes `sync_data()` on the file. Reads are page-cache eligible and include payload verification. The output measures this filesystem path, including its caching and writeback behavior. The size budget covers application payload bytes, and filesystem metadata and device writeback can add physical I/O. `--timeout-ms` accepts 1–60000 and defaults to 10000.

## TCP loopback

```sh
./target/release/embedded-linux-benchmark network --size-mib 8 --timeout-ms 5000
```

The command connects two TCP sockets through a listener bound to `127.0.0.1` on an ephemeral port. Timing starts after connection setup and covers one-way application throughput, including receiving and verifying the payload. The result covers the local TCP stack. Physical NIC throughput requires a separate measurement. The command uses the system's existing loopback configuration. `--size-mib` accepts 1–64 and defaults to 8. `--timeout-ms` accepts 1–30000 and defaults to 5000.

### TCP peer throughput and round-trip latency

Run these commands on two machines on an isolated wired or Wi-Fi test network. Use the numeric IPv4 address assigned to the intended interface on each machine. Agree on the port, session value, payload size, and probe count. The example uses addresses in a private subnet.

```sh
# Receiver
./target/release/embedded-linux-benchmark network peer-server --bind 192.168.5.10 \
  --peer 192.168.5.11 --port 5555 --session 0123456789abcdef0123456789abcdef
# Sender
./target/release/embedded-linux-benchmark network peer-client --bind 192.168.5.11 \
  --peer 192.168.5.10 --port 5555 --session 0123456789abcdef0123456789abcdef
```

The receiver accepts one connection from the named peer. The sender binds its source address. Both processes check the matching 32-hex-character session and budgets. The session identifies a test and provides no cryptographic protection. Public addresses, names, wildcard addresses, multicast, and addresses ending in `.0` or `.255` are refused. The receiver can use `--port 0` to print a kernel-assigned port for a local fixture.

`--size-mib` accepts 1–64 (default 8), `--pings` accepts 1–1000 (default 20), and `--timeout-ms` accepts 1–30000 (default 30000). The deadline includes connection setup or waiting for the sender. Latency is the application's TCP echo round trip. The receiver then verifies a one-way payload and reports throughput. The sender times the transfer through the final acknowledgment. Swap roles for the reverse path. Linux routing and the selected addresses determine the actual path. The command does not identify the physical link or measure wire-level latency.

A peer fixture using two distinct loopback addresses passed natively and with the OpenWrt 25.12.6 AArch64 APK and 24.10.8 MIPS little-endian IPK binaries under QEMU user mode. The same test passed with Jetson Linux 39.2.2 SDK and sample libraries. Hardware Ethernet and Wi-Fi measurements remain unverified.

## Peripherals

UART, I2C, SPI, and GPIO run only when requested directly. The automatic stress suite never accesses them.

### UART loopback

Use a dedicated fixture with the selected port's TX connected to its RX at the voltage and signaling levels specified for that port. Across separate connectors, share a ground and use compatible levels. Keep the boot console, login terminals, and any attached equipment out of the fixture. Use TX/RX only for the loopback. RS-232 signaling requires a suitable transceiver before it can connect to a logic-level UART.

```sh
./target/release/embedded-linux-benchmark uart --device /dev/ttyUSB0 --loopback --bytes 4096
```

`--loopback` acknowledges the dedicated fixture. The command checks the Linux console records, rejects standard streams and the controlling terminal, and accepts recognized `/dev` serial tty nodes such as `ttyUSB`, `ttyS`, and Jetson `ttyTHS` nodes, their aliases, or `/dev/pts/N` for testing. It requires the standard tty line discipline and a nonzero existing baud rate. The benchmark uses 115200 baud, 8 data bits, no parity, one stop bit, and no flow control. `--bytes` accepts 1–65536 and defaults to 4096. `--timeout-ms` accepts 1–30000 and defaults to 10000.

The result is verified payload bytes per second over the loopback, including both userspace I/O and comparison. The command opens the device nonblocking, leaves the original hang-up-on-close flag (`HUPCL`) intact, and restores the original tty settings on success or a handled error. A driver can still change modem lines when opening a port or when closing the last descriptor, depending on its existing settings. Use a fixture that does not rely on modem lines. Forced termination can interrupt settings restoration. The deadline cannot preempt a kernel driver call that stalls.

The UART implementation uses the Linux tty ABI for x86, x86-64, Arm, AArch64, and MIPS32 targets with glibc or musl. Other Linux targets return an explicit UART error before accessing a device. [Linux device numbers](https://docs.kernel.org/admin-guide/devices.html) · [Generic kernel termios ABI](https://github.com/torvalds/linux/blob/v6.12/include/uapi/asm-generic/termbits.h) · [MIPS kernel termios ABI](https://github.com/torvalds/linux/blob/v6.12/arch/mips/include/uapi/asm/termbits.h)

### I2C fixture read latency

Use a dedicated adapter and a fixture documented to support standalone reads of the selected length at the selected address. Reads can have side effects, including consuming queued data, clearing status, or advancing a device pointer. Choose an isolated fixture without equipment that relies on that device state.

```sh
./target/release/embedded-linux-benchmark i2c --adapter /dev/i2c-1 --address 0x48 \
  --fixture --read-bytes 2 --samples 100
```

The adapter must expose Linux `i2c-dev` with raw I2C support. Enter one unreserved 7-bit address from `0x08` through `0x77` in decimal or hexadecimal. The command uses the kernel's ordinary address selection and respects a busy address. Each sample issues exactly one fixed-length read. The command has no address scan, forced selection, write transaction, or register selection. It reports received bytes and mean userspace read latency. `--read-bytes` accepts 1–32 and defaults to 1. `--samples` accepts 1–1024 and defaults to 32. [Linux I2C userspace interface](https://docs.kernel.org/i2c/dev-interface.html)

### SPI fixture throughput

Connect the controller's MOSI to its MISO on a dedicated loopback fixture with compatible signal levels. Keep other peripherals disconnected from that bus. The controller can toggle clock and chip select during the test. Use an existing Linux `spidev` node with an 8-bit, single-lane full-duplex configuration.

```sh
./target/release/embedded-linux-benchmark spi --device /dev/spidev0.0 --loopback --bytes 4096
```

The command uses the existing SPI mode and transfer settings and reports the configured maximum clock. It does not write mode, word size, or speed settings. Each full-duplex transfer is at most 1024 bytes, and the result counts only received bytes that match the sent payload. `--bytes` accepts 1–65536 and defaults to 4096. The reported userspace rate includes transfers and verification. [Linux spidev interface](https://docs.kernel.org/spi/spidev.html)

### GPIO input read latency (legacy sysfs)

Use the Linux sysfs GPIO number of a line already exported and configured as an input. The example assumes an existing `gpio23` input. Sysfs GPIO is a legacy interface and may be missing from the target kernel. The benchmark requires a readable `direction` attribute equal to `in` and a readable `value` attribute. It opens both read-only and leaves export, pin direction, and pin state under the board's existing configuration. Keep the line exported as an input throughout the run. [Linux sysfs GPIO ABI](https://docs.kernel.org/userspace-api/gpio/sysfs.html)

```sh
./target/release/embedded-linux-benchmark gpio --pin 23 --samples 1000
```

`--samples` accepts 1–100000 and defaults to 1000. `--timeout-ms` accepts 1–30000 and defaults to 5000. The command reports the mean time for userspace to rewind and read one value, along with the number of high samples. Electrical propagation and edge timing require separate instrumentation. The deadline is checked between samples and cannot interrupt a stalled kernel GPIO read.

### GPIO input read latency (chardev v2)

Select an available spare input by its character-device chip and chip-relative line offset. The command checks the kernel-reported direction, requests the line exclusively as-is, then checks the direction again before reading. It leaves direction, bias, and logical polarity as configured, respects a line already in use, and releases the line descriptor at exit. The result reports userspace value-read latency and counts logical active values. The kernel controls the line's default state after release. [Linux GPIO v2 line requests](https://docs.kernel.org/userspace-api/gpio/gpio-v2-get-line-ioctl.html)

```sh
./target/release/embedded-linux-benchmark gpio --chip /dev/gpiochip0 --offset 7 \
  --input-fixture --samples 1000
```

The chardev mode uses the same sample limits as legacy GPIO. It requires the GPIO v2 ABI, which entered mainline in Linux 5.10 and depends on the target kernel configuration. A legacy-only kernel can use `--pin` if the sysfs input exists. The two GPIO modes require separate invocations. [Linux GPIO chardev API](https://docs.kernel.org/userspace-api/gpio/chardev.html)

I2C, SPI, and GPIO chardev accept `--timeout-ms` from 1–30000 with a 5000 ms default. Each run is foreground and checks its monotonic deadline between operations. A blocked kernel driver call can delay that check. These three commands require sysfs device identity records and the Linux device ABI on x86, x86-64, Arm, AArch64, or MIPS32 with glibc or musl. Other ABIs fail before accessing the device.

## Automatic stress

```sh
./target/release/embedded-linux-benchmark stress auto
./target/release/embedded-linux-benchmark stress auto --rounds 2 --duration-ms 500 --network-mib 4
./target/release/embedded-linux-benchmark stress auto --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

By default each round runs CPU, RAM, and loopback in sequence. Supplying both `--storage-dir` and `--storage-mib` adds storage after loopback. `--storage-mib` is the total 1–64 MiB write budget across all rounds. The suite divides it among rounds, with any remaining bytes in the final round. `--storage-sync` is optional with storage enabled.

Use one `--test` selector to repeat a single component. Each selection accepts `--rounds` and `--timeout-ms`. CPU and RAM accept `--duration-ms`, RAM accepts `--buffer-kib`, and network accepts `--network-mib`. Storage-only requires both the directory and total write budget and accepts `--storage-sync`. Options for other components produce an error before the run starts.

```sh
./target/release/embedded-linux-benchmark stress auto --test cpu --rounds 3 --duration-ms 500
./target/release/embedded-linux-benchmark stress auto --test ram --buffer-kib 2048
./target/release/embedded-linux-benchmark stress auto --test network --network-mib 4
./target/release/embedded-linux-benchmark stress auto --test storage --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

| Option | Default | Accepted values |
| --- | --- | --- |
| `--rounds` | 3 | 1–5 |
| `--duration-ms` | 250 per CPU or RAM phase | 1–5000 |
| `--buffer-kib` | 1024 per RAM buffer | 4–32768 |
| `--network-mib` | 8 per round | 1–64 |
| `--timeout-ms` | 15000 for the full run | 1–120000, greater than the total duration of all scheduled CPU and RAM phases |

### Elapsed-time runs

Use `--duration-secs` for a foreground run with a total elapsed target from 1 through 1209600 seconds (14 days). It applies to the suite or any individual `--test`. This mode replaces `--rounds` and `--timeout-ms`. `--duration-ms` still sets the CPU or RAM phase length, and the last phase may run for less time as the deadline approaches.

```sh
./target/release/embedded-linux-benchmark stress auto --duration-secs 60
./target/release/embedded-linux-benchmark stress auto --test cpu --duration-secs 1209600
./target/release/embedded-linux-benchmark stress auto --test ram --duration-secs 300 --duration-ms 500
./target/release/embedded-linux-benchmark stress auto --test storage --duration-secs 300 \
  --storage-dir /path/to/writable-directory --storage-mib 16
```

Elapsed-time runs keep fixed-size totals, emit progress at most once every five minutes, and print the measured elapsed time and stop reason. CPU and RAM totals include shortened measured phases. Network and verified storage totals include completed operations. The storage write count includes bytes written before a cutoff. Loopback operations retain their configured byte cap and a five-second timeout. Each storage operation uses at most 1 MiB of the explicit total write budget and a ten-second deadline. These operation deadlines are also limited by the remaining run time.

The storage write budget is shared across the entire run at any duration. The suite announces when it reaches that budget and continues the other selected phases. Storage-only finishes early after verifying its final write. The stress suite measures workload activity and leaves thermal and throttling controls to the operating system and firmware.

The suite validates settings before starting and stops at the first failure. An elapsed-time run succeeds when it reaches its requested duration or completes a storage-only write budget. A failed component, a component operation timeout before the run deadline, or a round-based run exceeding its timeout produces a nonzero exit status. The RAM working set is bounded by two buffers, with a maximum of 64 MiB. Storage and network use fixed-size application buffers.

The elapsed-time CPU summary measures workload completion and reports total iterations. The timed suite performs no CPU checksum comparison. The standalone `cpu` command reports its checksum, and the standalone CLI tests independently recompute it from the measured iteration count.

Deadlines are checked during the run between CPU batches, RAM copies, I/O chunks, and phases. An in-progress filesystem call such as `sync_data()`, time spent waiting for the OS scheduler, or allocation and page faults can delay the next check. Sudden termination, including default `SIGINT` handling, or power loss can leave a `.embedded-linux-benchmark-*` file in the selected directory.

## Validation

The native Debian 13 release tests cover component output, loopback byte counts, invalid limits and incomplete storage opt-ins, storage budget accounting, and scratch-file removal. A child process with a small file-size limit exercises a write failure both directly and in the stress suite. The default three-round run also passed without creating a scratch file.

All four individual selectors passed native CLI tests, an OpenWrt 25.12.6 AArch64 full-system QEMU test, and Jetson Linux 39.2.2 QEMU user-mode checks with the matching NVIDIA sample libraries. These runs checked the selected phase count, storage byte accounting and cleanup, and rejection of conflicting settings.

Short elapsed-time runs passed on native Debian 13, in matching OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS32 little-endian QEMU guests, and under QEMU user mode with Jetson Linux 39.2.2 sample libraries. They exercised deadline clamping, the shared storage budget, storage-only early exit, and scratch cleanup. The 14-day boundary and overflow were tested through argument parsing. The full duration was not run.

UART pseudoterminal tests passed natively and in the Jetson Linux 39.2.2 QEMU user-mode setup. They exercised loopback byte counts, corruption, timeout, terminal refusal, and tty restoration. The GPIO unit test uses ordinary files as inert attributes to check input-only reads, counts, direction and type rejection, and timeout.

```sh
cargo test --release --locked
cargo build --release --locked
python3 tests/uart_pty.py ./target/release/embedded-linux-benchmark
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

### I2C, SPI, and GPIO chardev verification

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

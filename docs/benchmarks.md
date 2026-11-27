# Storage, network, peripherals, and automatic stress

Use `cargo build --release --locked` and run `./target/release/eld-engine --help` for the commands. All workloads run on the calling thread. The `cpu` and `ram` commands also run independently with their existing settings.

## Bring-up inventory

`eld-engine inventory` prints a read-only JSON summary of process architecture,
Linux kernel release, distribution, and model or firmware fields exposed by
Device Tree or DMI. It does not scan the network, read machine IDs or serial
numbers, or execute a system configuration file. The architecture describes
the executable, not a claim about the physical board under emulation.
Unavailable, inaccessible, and invalid observations stay separate.

## CPU and RAM

```sh
eld-engine cpu --duration-ms 500
eld-engine ram --duration-ms 500 --buffer-kib 1024
```

The CPU test measures single-threaded 32-bit integer throughput. The RAM test
copies between two buffers of the selected size. Both are active local loads:
use short runs on a lab board and interpret results in the context of its power,
cooling and firmware configuration. See [automatic stress](#automatic-stress) for
a longer or combined run.

## Storage

Select an existing writable directory and a size of 1–64 MiB. The command creates a uniquely named regular file with owner-only permissions, writes the requested payload sequentially, then reads and verifies the same byte count. Exclusive creation protects existing files. The command removes the scratch file on success or a handled error and reports the path if removal fails.

```sh
./target/release/eld-engine storage --dir /path/to/writable-directory --size-mib 16
./target/release/eld-engine storage --dir /path/to/writable-directory --size-mib 16 --sync
```

The write and read rates are separate. Writes go through buffered filesystem I/O. With `--sync`, write timing includes `sync_data()` on the file. Reads are page-cache eligible and include payload verification. The output measures this filesystem path, including its caching and writeback behavior. The size budget covers application payload bytes, and filesystem metadata and device writeback can add physical I/O. `--timeout-ms` accepts 1–60000 and defaults to 10000.

## TCP loopback

```sh
./target/release/eld-engine network --size-mib 8 --timeout-ms 5000
```

The command connects two TCP sockets through a listener bound to `127.0.0.1` on an ephemeral port. Timing starts after connection setup and covers one-way application throughput, including receiving and verifying the payload. The result covers the local TCP stack. Physical NIC throughput requires a separate measurement. The command uses the system's existing loopback configuration. `--size-mib` accepts 1–64 and defaults to 8. `--timeout-ms` accepts 1–30000 and defaults to 5000.

### TCP peer throughput and round-trip latency

Run these commands on two machines on an isolated wired or Wi-Fi test network. Use the numeric IPv4 address assigned to the intended interface on each machine. Agree on the port, session value, payload size, and probe count. The example uses addresses in a private subnet.

```sh
# Receiver
./target/release/eld-engine network peer-server --bind 192.168.5.10 \
  --peer 192.168.5.11 --port 5555 --session 0123456789abcdef0123456789abcdef
# Sender
./target/release/eld-engine network peer-client --bind 192.168.5.11 \
  --peer 192.168.5.10 --port 5555 --session 0123456789abcdef0123456789abcdef
```

The receiver accepts one connection from the named peer. The sender binds its source address. Both processes check the matching 32-hex-character session and budgets. The session identifies a test and provides no cryptographic protection. Public addresses, names, wildcard addresses, multicast, and addresses ending in `.0` or `.255` are refused. The receiver can use `--port 0` to print a kernel-assigned port for a local fixture.

`--size-mib` accepts 1–64 (default 8), `--pings` accepts 1–1000 (default 20), and `--timeout-ms` accepts 1–30000 (default 30000). The deadline includes connection setup or waiting for the sender. Latency is the application's TCP echo round trip. The receiver then verifies a one-way payload and reports throughput. The sender times the transfer through the final acknowledgment. Swap roles for the reverse path. Linux routing and the selected addresses determine the actual path. The command does not identify the physical link or measure wire-level latency.

A peer fixture using two distinct loopback addresses passed natively and with the OpenWrt 25.12.6 AArch64 APK and 24.10.8 MIPS little-endian IPK binaries under QEMU user mode. The same test passed with Jetson Linux 39.2.2 SDK and sample libraries. Hardware Ethernet and Wi-Fi measurements remain unverified.

## Peripherals

Active peripheral benchmarks run only when requested directly. Automatic stress only observes allowlisted PCIe, USB and HDMI status attributes in sysfs; it never opens a peripheral device node or initiates an external test.

### PCIe link information

```sh
./target/release/eld-engine pcie link --device 0000:01:00.0
```

Use the complete PCI domain, bus, device and function for the device you intend to observe. The command reads the kernel's negotiated and maximum link speed and lane width for that device. Missing attributes are reported as unavailable. The speeds are link signaling rates in GT/s, and the maximum values are capabilities. Measuring payload throughput requires a separate test. [Linux PCI sysfs implementation](https://github.com/torvalds/linux/blob/v6.12/drivers/pci/pci-sysfs.c)

`pcie throughput --device BDF --fixture` returns an unavailable error before opening a device. The published stock endpoint-test read, write and copy requests specify a transfer size and flags. The reviewed Linux 6.12 and 6.18 host drivers wait for IRQ completion without a transfer timeout. They perform completion or integrity checks but expose no per-direction timing or completed byte counter. A userspace timeout cannot interrupt that wait. A bounded throughput benchmark requires an identifiable driver ABI with a completion deadline and an exact dedicated endpoint fixture. [Linux 6.12 endpoint-test UAPI](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/pcitest.h) · [Linux 6.18 endpoint-test driver](https://github.com/torvalds/linux/blob/v6.18/drivers/misc/pci_endpoint_test.c)

### Read host resource inventory

`eld-engine inventory` also reports online CPU and NUMA node counts and the
kernel's total and available memory. Measurements come from the visible Linux
sysfs and procfs, not container resource limits or a performance test. Observed
values are decimal strings; absent or unreadable files are not zero counts.
The thermal section reports zone type and temperature in millidegrees Celsius,
with at most 32 thermal zones and explicit truncation or unavailable status.
It reads recognized sysfs thermal attributes without changing trips, fans, or policy.
Add `--thermal-sample-ms 1000` for two passive snapshots and per-zone
temperature differences (1–60,000 ms). A replaced, unlabelled or unavailable
zone is not comparable. The interval is monotonic, and no load is started.
The memory EDAC section reads per-controller corrected and uncorrected counters
when exposed by Linux, plus the optional seconds since initialization or reset.
DIMM or unattributed breakdowns are not added to controller totals. A snapshot
is not proof of continuity between reads; an empty controller
list is not proof of zero errors or that the hardware lacks ECC. The Engine never
resets counters or injects errors.
CPU frequency observations list up to 32 kernel policies and distinguish
scaling-reported frequency from hardware-reported current frequency when
available. Limits and changing clocks are not evidence of thermal throttling.
Where the kernel exports thermal event counters, the Engine reports
per-logical-CPU observations. Package events can be replicated across CPUs and
are never summed. They are cumulative observations, not current throttle state.
Powercap observations report separate parent and child energy counters and their
wrap range in microjoules where accessible. They are not instantaneous watts;
zones can overlap and are never summed. No power limits or counters are changed.

### Check expected PCI devices

Use an explicit list from a known board configuration to check PCI enumeration:

```sh
eld-engine inventory --require-pcie 0000:01:00.0=8086:1234,0000:02:00.0
```

This passive check accepts up to 16 unique bus addresses. It reports each as present,
missing, or indeterminate (unavailable, inaccessible, invalid, or changed). A missing
entry requires an available PCI device index. A present entry means the address was
enumerated, not that the expected hardware or driver is healthy. PCI addresses can
change with firmware or hardware configuration. Read the JSON statuses; an absent
device does not by itself make the command exit nonzero. A row with optional expected
vendor and device IDs instead reports `match` or `mismatch` if both IDs are readable,
with expected and observed values; unreadable IDs are indeterminate, not a mismatch.

### Reuse a board profile

Save the following three lines as a regular file and run
`eld-engine inventory --board-profile board.profile`:

```text
eld-board-profile=1
compatible=examplevendor,example-board
require-pcie=0000:01:00.0=8086:1234,0000:02:00.0
```

Replace the example with the exact first root Device Tree `compatible` value and
required devices for the known board. The profile is applied only when that first
value matches; a later shared SoC entry does not qualify. Missing or generic
board evidence produces `unknown` and skips profile PCI checks. This file is
data only, not a script, and cannot be combined with `--require-pcie`.
The `board_check` summary counts passed, failed, skipped and unavailable
requirements. No PCI check is scored for a different or unknown board. A pass
only covers the profile’s passive PCI identity requirements; it is not
performance, reliability or production qualification.

### Save an inventory evidence file

```sh
eld-engine inventory --board-profile board.profile --evidence-json ./inventory-evidence.json
```

The optional file uses evidence schema v1 and contains the same inventory
object printed to stdout, wall-clock start and finish in Unix milliseconds,
and monotonic elapsed milliseconds. Wall-clock values are device-reported,
may drift or move backwards, and are null for times before the Unix epoch.
The parent directory must exist. Use a private directory: the file is created
with owner-only access and never replaces an existing file or follows a
final-component symlink. Check the command’s exit status before consuming the
file; an interrupted or failed write may leave a partial file. This passive
evidence is not a signed attestation or hardware qualification.

### PCIe health snapshot

```sh
./target/release/eld-engine pcie health --device 0000:01:00.0
```

Writes one versioned JSON object to stdout with the target and its upstream PCI devices. The command reads only allowlisted Linux sysfs attributes; it does not start a workload or change device state. Per-device AER message totals and named categories are separate from root-port hierarchy totals. Counters are decimal strings to preserve exact 64-bit values. An unavailable, inaccessible, or invalid attribute is not a zero. A single snapshot is cumulative, not a before/after measurement or proof that a specific component is faulty.

Each PCI device row reports its observed driver binding. A bound driver can legitimately have no visible module; this does not indicate whether it was built in or separately loaded. A changing or dangling link is not reported as a stable binding.

Use `--sample-ms 1000` to take a second snapshot after a bounded wait (1–60000 ms). No load is generated. The JSON contains both snapshots, the measured interval between sample starts, and a per-observer delta only when boot and sysfs identities match. A decrease, a changed category set, or an unavailable counter never becomes an invented zero. Counter deltas show what changed during the interval, not which component caused it.

To assess a board's expected link, add `--min-width 4 --min-speed-gtps 8.0`.
The optional `target_link_expectation` object compares only the selected device's
negotiated values. It reports `below_minimum`, `meets_minimum`, or an observation
state such as `unavailable`. Device maximum capabilities are not used as expected
board wiring. A shortfall is evidence for investigation, and does not on its own
change the command's exit status.

### IIO raw input cadence

```sh
./target/release/eld-engine iio --device iio:device0 --channel in_voltage0 --samples 100
```

Select an available input channel by its sysfs stem. This example reads `in_voltage0_raw` as a signed integer. Use a spare input where repeated reads are appropriate. The command rewinds and reads the same attribute for every sample. It reports the raw range and mean, mean userspace seek-plus-read time, and cadence and standard deviation between successive read completions.

`--samples` accepts 2–10000 with a default of 100. `--timeout-ms` accepts 1–30000 with a default of 5000. The command checks the deadline between reads. A stalled driver call can extend the run. These observations use userspace monotonic time. Buffered hardware timestamps, converter sample rates and calibrated units require device-specific information. The benchmark performs read-only access to the selected raw attribute. [Linux IIO sysfs ABI](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-bus-iio)

PCIe and IIO validation used inert sysfs fixtures and CLI rejection tests. OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS little-endian SDK packages built successfully. CLI rejection passed in those userspaces and in the Jetson Linux 39.2.2 sample userspace under QEMU user mode.

### Camera delivery rate

Use an explicit V4L2 video node from a dedicated, already configured capture source. The command reports the driver's current dimensions, pixel format, plane count, field mode, and frame interval when available. It uses the streaming MMAP queue, with single-plane and multi-plane capture support, and discards buffers without saving or recording frames. It leaves the device's format and controls as configured.

```sh
./target/release/eld-engine camera --device /dev/video0 --frames 120
```

`--frames` accepts 2–10000 (default 120). `--timeout-ms` accepts 1–30000 (default 10000). The command asks the driver for four buffers and limits each mapped buffer to 24 MiB and the total mappings to 96 MiB. The deadline is checked between operations. A stalled driver call can extend the run. Capture currently uses the verified 64-bit V4L2 buffer ABI on Linux x86-64 and AArch64 with glibc or musl. Other ABIs return a clear error before opening a node. [Linux V4L2 streaming API](https://docs.kernel.org/userspace-api/media/v4l/mmap.html)

The delivery rate and interval standard deviation use monotonic userspace dequeue times. Alternating-field mode reports fields per second. Scheduler delays and buffering affect the measurement. Driver sequence gaps and buffers flagged erroneous are reported separately. Gaps can miss frame loss before the driver assigns a sequence, and driver resets or repeated sequences can make gap counts incomplete. [Linux V4L2 buffer semantics](https://docs.kernel.org/userspace-api/media/v4l/buffer.html)

### HDMI connector observations and capture delivery

```sh
./target/release/eld-engine hdmi status --connector card0-HDMI-A-1
./target/release/eld-engine hdmi capture --device /dev/video2 --fixture --frames 120
```

`hdmi status` reads the explicit DRM sysfs connector's connection status, encoder state, and advertised mode names. Those attributes do not expose active timing. The command does not set display modes. [Linux DRM connector attributes](https://github.com/torvalds/linux/blob/v6.12/drivers/gpu/drm/drm_sysfs.c)

`hdmi capture` uses the camera streaming benchmark on an explicitly identified, dedicated HDMI capture device. The result covers delivery rate and jitter at that device. Output-to-capture latency requires a synchronized stimulus and reference. Verification used simulated V4L2 transactions and inert connector attributes. Physical video and HDMI results remain unverified.

V4L2 ABI checks passed with native Linux and the OpenWrt AArch64 and Jetson SDK headers. The Jetson Linux 39.2.2 unit tests exercised mock multi-plane queueing, dequeue, and cleanup under QEMU user mode. OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS32 packages built successfully, and CLI smoke tests in matching userspaces checked device and fixture rejection, including the explicit 32-bit capture error.

### UART loopback

Use a dedicated fixture with the selected port's TX connected to its RX at the voltage and signaling levels specified for that port. Across separate connectors, share a ground and use compatible levels. Keep the boot console, login terminals, and any attached equipment out of the fixture. Use TX/RX only for the loopback. RS-232 signaling requires a suitable transceiver before it can connect to a logic-level UART.

```sh
./target/release/eld-engine uart --device /dev/ttyUSB0 --loopback --bytes 4096
```

`--loopback` acknowledges the dedicated fixture. The command checks the Linux console records, rejects standard streams and the controlling terminal, and accepts recognized `/dev` serial tty nodes such as `ttyUSB`, `ttyS`, and Jetson `ttyTHS` nodes, their aliases, or `/dev/pts/N` for testing. It requires the standard tty line discipline and a nonzero existing baud rate. The benchmark uses 115200 baud, 8 data bits, no parity, one stop bit, and no flow control. `--bytes` accepts 1–65536 and defaults to 4096. `--timeout-ms` accepts 1–30000 and defaults to 10000.

The result is verified payload bytes per second over the loopback, including both userspace I/O and comparison. The command opens the device nonblocking, leaves the original hang-up-on-close flag (`HUPCL`) intact, and restores the original tty settings on success or a handled error. A driver can still change modem lines when opening a port or when closing the last descriptor, depending on its existing settings. Use a fixture that does not rely on modem lines. Forced termination can interrupt settings restoration. The deadline cannot preempt a kernel driver call that stalls.

The UART implementation uses the Linux tty ABI for x86, x86-64, Arm, AArch64, and MIPS32 targets with glibc or musl. Other Linux targets return an explicit UART error before accessing a device. [Linux device numbers](https://docs.kernel.org/admin-guide/devices.html) · [Generic kernel termios ABI](https://github.com/torvalds/linux/blob/v6.12/include/uapi/asm-generic/termbits.h) · [MIPS kernel termios ABI](https://github.com/torvalds/linux/blob/v6.12/arch/mips/include/uapi/asm/termbits.h)

### Classic CAN and CAN FD peer fixture

Use two endpoints on a dedicated isolated CAN bus with matching existing bitrate and termination. Keep the fixture disconnected from production equipment. Agree on two distinct standard 11-bit identifiers, an eight-character hexadecimal session, and a frame count. Start the receiver first.

```sh
# Receiver
./target/release/eld-engine can receive --interface can0 --tx-id 0x124 \
  --rx-id 0x123 --session 1234abcd --fixture --frames 128
# Sender
./target/release/eld-engine can send --interface can0 --tx-id 0x123 \
  --rx-id 0x124 --session 1234abcd --fixture --frames 128
```

Both sides use 8-byte Classic CAN by default. Add `--fd` to both for 64-byte CAN FD frames on interfaces already configured for CAN FD. This benchmark uses no bit-rate switching or interface configuration. Socket filters select the receive ID and local echo is disabled. Every echoed sequence and payload is verified. The sender reports completed round-trip cycles per second and mean cycle time. `--frames` accepts 1–4096 (default 128). `--timeout-ms` accepts 1–30000 (default 10000), including time waiting for a peer. [Linux SocketCAN](https://docs.kernel.org/networking/can.html)

Verification covers Classic CAN and CAN FD wire formats and round trips with mock I/O. Native and SDK header checks include MIPS. Physical CAN traffic and performance remain unverified.

### USB bulk loopback

Use an isolated USB fixture with firmware that echoes each bulk OUT transfer onto its bulk IN endpoint. Select the device node, current interface number, and both endpoint addresses explicitly. The interface must be unbound from kernel drivers and expose both bulk endpoints in its current configuration and alternate setting.

```sh
./target/release/eld-engine usb --device /dev/bus/usb/001/011 \
  --interface 0 --out-endpoint 0x01 --in-endpoint 0x81 --loopback --bytes 4096
```

The command claims the selected interface, uses bounded usbfs bulk OUT and IN transfers of at most 4096 bytes each, verifies the echo, and releases the interface. It exposes no control messages, resets, configuration changes, or driver detachment. `--bytes` accepts 1–65536 (default 4096). `--timeout-ms` accepts 1–30000 (default 5000). Each request uses the remaining run time as its nonzero kernel timeout. The result includes both directions and verification. [Linux USB userspace API](https://docs.kernel.org/driver-api/usb/usb.html)

USB validation used an inert sysfs interface fixture and mocked transfers, including short sends, corrupted replies, and a deadline. Native and SDK ABI checks covered 64-bit, Armv7, and both MIPS byte orders. Package install, peer TCP, USB and CAN rejection, and package removal passed in disposable OpenWrt 21.02.7 AArch64, 25.12.6 AArch64, and 24.10.8 MIPS little-endian guests. USB and CAN fixture unit tests also passed under Jetson Linux 39.2.2 QEMU user mode. Real USB transfers remain unverified.

### I2C fixture read latency

Use a dedicated adapter and a fixture documented to support standalone reads of the selected length at the selected address. Reads can have side effects, including consuming queued data, clearing status, or advancing a device pointer. Choose an isolated fixture without equipment that relies on that device state.

```sh
./target/release/eld-engine i2c --adapter /dev/i2c-1 --address 0x48 \
  --fixture --read-bytes 2 --samples 100
```

The adapter must expose Linux `i2c-dev` with raw I2C support. Enter one unreserved 7-bit address from `0x08` through `0x77` in decimal or hexadecimal. The command uses the kernel's ordinary address selection and respects a busy address. Each sample issues exactly one fixed-length read. The command has no address scan, forced selection, write transaction, or register selection. It reports received bytes and mean userspace read latency. `--read-bytes` accepts 1–32 and defaults to 1. `--samples` accepts 1–1024 and defaults to 32. [Linux I2C userspace interface](https://docs.kernel.org/i2c/dev-interface.html)

### SPI fixture throughput

Connect the controller's MOSI to its MISO on a dedicated loopback fixture with compatible signal levels. Keep other peripherals disconnected from that bus. The controller can toggle clock and chip select during the test. Use an existing Linux `spidev` node with an 8-bit, single-lane full-duplex configuration.

```sh
./target/release/eld-engine spi --device /dev/spidev0.0 --loopback --bytes 4096
```

The command uses the existing SPI mode and transfer settings and reports the configured maximum clock. It does not write mode, word size, or speed settings. Each full-duplex transfer is at most 1024 bytes, and the result counts only received bytes that match the sent payload. `--bytes` accepts 1–65536 and defaults to 4096. The reported userspace rate includes transfers and verification. [Linux spidev interface](https://docs.kernel.org/spi/spidev.html)

### GPIO input read latency (legacy sysfs)

Use the Linux sysfs GPIO number of a line already exported and configured as an input. The example assumes an existing `gpio23` input. Sysfs GPIO is a legacy interface and may be missing from the target kernel. The benchmark requires a readable `direction` attribute equal to `in` and a readable `value` attribute. It opens both read-only and leaves export, pin direction, and pin state under the board's existing configuration. Keep the line exported as an input throughout the run. [Linux sysfs GPIO ABI](https://docs.kernel.org/userspace-api/gpio/sysfs.html)

```sh
./target/release/eld-engine gpio --pin 23 --samples 1000
```

`--samples` accepts 1–100000 and defaults to 1000. `--timeout-ms` accepts 1–30000 and defaults to 5000. The command reports the mean time for userspace to rewind and read one value, along with the number of high samples. Electrical propagation and edge timing require separate instrumentation. The deadline is checked between samples and cannot interrupt a stalled kernel GPIO read.

### GPIO input read latency (chardev v2)

Select an available spare input by its character-device chip and chip-relative line offset. The command checks the kernel-reported direction, requests the line exclusively as-is, then checks the direction again before reading. It leaves direction, bias, and logical polarity as configured, respects a line already in use, and releases the line descriptor at exit. The result reports userspace value-read latency and counts logical active values. The kernel controls the line's default state after release. [Linux GPIO v2 line requests](https://docs.kernel.org/userspace-api/gpio/gpio-v2-get-line-ioctl.html)

```sh
./target/release/eld-engine gpio --chip /dev/gpiochip0 --offset 7 \
  --input-fixture --samples 1000
```

The chardev mode uses the same sample limits as legacy GPIO. It requires the GPIO v2 ABI, which entered mainline in Linux 5.10 and depends on the target kernel configuration. A legacy-only kernel can use `--pin` if the sysfs input exists. The two GPIO modes require separate invocations. [Linux GPIO chardev API](https://docs.kernel.org/userspace-api/gpio/chardev.html)

I2C, SPI, and GPIO chardev accept `--timeout-ms` from 1–30000 with a 5000 ms default. Each run is foreground and checks its monotonic deadline between operations. A blocked kernel driver call can delay that check. These three commands require sysfs device identity records and the Linux device ABI on x86, x86-64, Arm, AArch64, or MIPS32 with glibc or musl. Other ABIs fail before accessing the device.

## Automatic stress

```sh
./target/release/eld-engine stress auto
./target/release/eld-engine stress auto --rounds 2 --duration-ms 500 --network-mib 4
./target/release/eld-engine stress auto --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

Each automatic round overlaps three sustained workloads: one integer CPU worker, RAM copies, and verified TCP loopback transfers. Workers keep running for at least the configured phase length, subject to the global deadline. Loopback reuses one connection within a round and continuously transfers data after reaching the initial minimum byte count. There is no sleep inserted between rounds.

The suite also reads available PCIe link width, USB link speed, and HDMI encoder status once per round. These are **passive observations, not active hardware benchmarks**. Missing or inaccessible attributes are reported as unavailable or skipped. Device discovery is capped at 256 entries per class and 32 candidates per class, and it runs only once at startup. Camera, bus, GPIO/IIO, accelerator, and network peer workloads stay out of automatic stress because they require an explicit device, fixture, or peer. Use their standalone commands to run them.

Supplying both `--storage-dir` and `--storage-mib` adds bounded filesystem work alongside the other loads. `--storage-mib` is the **total** 1–64 MiB write budget across all rounds, not a per-round allowance. Fixed-round mode divides it among rounds, putting any remainder in the final round. Add `--storage-sync` to include `sync_data()`.

Use one `--test` selector to repeat a single component without passive observations. CPU, RAM and network accept `--duration-ms`, RAM accepts `--buffer-kib`, and network accepts `--network-mib`. Storage-only requires both the directory and the total write budget and accepts `--storage-sync`. Options for other components produce an error before the run starts.

```sh
./target/release/eld-engine stress auto --test cpu --rounds 3 --duration-ms 500
./target/release/eld-engine stress auto --test ram --buffer-kib 2048
./target/release/eld-engine stress auto --test network --network-mib 4
./target/release/eld-engine stress auto --test storage --rounds 2 \
  --storage-dir /path/to/writable-directory --storage-mib 16 --storage-sync
```

| Option | Default | Accepted values |
| --- | --- | --- |
| `--rounds` | 3 | 1–5 |
| `--duration-ms` | 10000 per CPU, RAM and loopback phase | 1–60000 |
| `--buffer-kib` | 1024 per RAM buffer (2048 total) | 4–32768 |
| `--network-mib` | 8 minimum per complete loopback phase | 1–64 |
| `--timeout-ms` | 60000 for the full fixed-round run | 1–360000, greater than round count × phase duration unless storage-only |

### Elapsed-time runs

Use `--duration TIME` or `--duration-secs SEC` for a foreground elapsed target from 1 second through 14 days. `TIME` accepts whole seconds, minutes, hours, days, or weeks, with suffixes `s`, `m`, `h`, `d` and `w`. Either duration option replaces `--rounds` and `--timeout-ms`.

```sh
./target/release/eld-engine stress auto --duration 30m
./target/release/eld-engine stress auto --duration 2w
./target/release/eld-engine stress auto --duration-secs 1209600
./target/release/eld-engine stress auto --test ram --duration 1h --duration-ms 500
./target/release/eld-engine stress auto --test storage --duration 5m \
  --storage-dir /path/to/writable-directory --storage-mib 16
```

A complete round finishes all active phases at their configured duration or byte count. Near the deadline, they still start together but the shortened final round is counted separately as partial, never as complete. Partial CPU/RAM work and payload-verified loopback bytes still contribute to totals. Storage accounts for bytes actually written even if readback did not finish before the deadline, and only completed readbacks contribute to verified bytes. A workload error or a per-operation timeout before the global deadline fails the run.

A timed run keeps fixed-size totals, prints progress at most once every five minutes, and reports the elapsed time and stop reason. CPU, RAM and loopback workers are reused between rounds. Loopback fails if it stalls for five seconds before the global deadline. A timed storage operation uses at most 1 MiB of the total write budget and a ten-second operation deadline, shortened by the global deadline. When the write budget is spent, storage is skipped in subsequent suite rounds; storage-only instead finishes early after verifying its final write. The RAM cap is two 32 MiB buffers. Other workload buffers and passive observations have fixed limits.

Run a soak on a board with adequate cooling and power. This is a foreground workload, not a watchdog or recovery service: firmware limits, suspend, reboot, power loss, and process termination can interrupt it; it does not restart automatically. The monotonic clock excludes system suspend on Linux. Deadlines are checked between CPU batches, RAM copies, I/O chunks, and phases. An OS-scheduled delay or in-progress kernel I/O such as `sync_data()` can delay a check; the tool does not forcibly stop a driver operation. Sudden termination can leave a `.eld-engine-*` scratch file in a selected storage directory.

The elapsed CPU summary counts work completed; it is not an independent CPU correctness proof. The standalone `cpu` command reports its checksum. The standalone CLI tests independently recompute it from the measured iteration count.

## Validation

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

#### Optional final JSON summary

Add `--summary-json ./stress-summary.json` to write a versioned summary with
the stop reason, completed rounds, elapsed duration and selected workload
totals. The command reserves a new private file before starting load, never
overwrites an existing file, and leaves normal reports unchanged. The file
is populated at the end; startup failure or interruption can leave it empty
or incomplete. Treat it as successful evidence only when the owned process
exited successfully and the summary says `completed`. Count and measured
nanosecond values are decimal strings. This is not a stream, a peripheral
qualification result, or evidence that every metric remained healthy.

Add `--events-ndjson ./stress-events.ndjson` for a separate private file with
one start line, at most one progress line per five minutes at a completed
round boundary, and one finish line. Missed intervals are not replayed.
Read only complete lines; an interruption or startup/I/O error can leave a
partial stream or no finish line. A finish line records the workload outcome,
not successful file delivery or process exit. Require a successful exit from
the owned command before accepting a complete run. Use a fresh path for each
attempt. This stream does not create any extra workload or device access.

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

### Passive network link health

Run `eld-engine network health --interface eth0` with the name of the interface
to inspect. The JSON snapshot shows carrier, negotiated speed when available,
duplex, Linux operational state, traffic and error/drop counters, and carrier
changes. Receive CRC errors and receive-buffer misses are reported separately
when available. These counters can help distinguish frame errors from receive
pressure, but do not identify the root cause. Linux can report an explicit
`unknown` operational state even when the attribute is readable. The command
reads only that interface's Linux sysfs observations and sends no traffic.
Counters are cumulative, read separately, and are not a bandwidth or
packet-loss test. See the kernel's [interface statistics](https://docs.kernel.org/networking/statistics.html)
and [operational states](https://docs.kernel.org/networking/operstates.html).

Add `--sample-ms 1000` to take two passive snapshots (allowed: 1–60,000 ms).
The report records the monotonic interval and whether the same boot and
interface identity were observed. It reports per-counter differences only with
comparable observations, and withholds all differences if a counter decreases.
Unchanged endpoint carrier states may still hide transitions recorded by
`carrier_changes`. Sysfs fields are read sequentially; the result is not a
precise rate, loss percentage, or proof that no transient reset occurred.
Boot identifiers are not printed.

### Passive NVMe controller status

`eld-engine storage nvme-health --controller nvme0` reads the named
controller's Linux sysfs state, transport, model and firmware revision.
It does not read serial numbers, SMART or media-error logs, open raw devices,
change controller state, or benchmark storage. A live controller state does
not certify drive health.


#### Decode a previously captured SMART log

`eld-engine storage nvme-smart-decode < captured-smart-log.bin` reads exactly
512 bytes of raw NVMe SMART / Health Information log page 02h from standard
input. Supply a finished file, not a live device path, a growing stream, or
text/JSON. The Engine does not collect the log or open an NVMe device.
The output includes warning bits, composite Kelvin temperature (zero left
uninterpreted as null), normalized spare fields (reserved values are null),
and the raw vendor endurance estimate, which can exceed 100. The 128-bit
counters are exact decimal strings. The data units read and written are
rounded-up groups of one thousand 512-byte units, not exact byte totals.
Their raw zero means unreported, unlike a zero critical-warning mask.
No health verdict is inferred. Keep capture time, controller identity, log
identifier, controller/namespace scope, and a digest of the raw page with
the capture; the raw page does not authenticate them. See the
[NVM Express Base Specification 2.0e](https://nvmexpress.org/wp-content/uploads/NVM-Express-Base-Specification-2.0e-2024.07.29-Ratified.pdf),
SMART / Health Information log.

### Passive Linux pressure snapshot

`eld-engine pressure` reads the CPU, memory and I/O files under
`/proc/pressure` once, without starting a workload or registering a kernel
trigger. The JSON preserves the three percentage windows as decimal strings
and exact cumulative stall time in microseconds. Missing resources are
separate from observed zero. System CPU `full` is not applicable even if
the kernel exposes its backward-compatibility zero. This is system-level
pressure, not a per-process measurement or a benchmark result.
See [Linux Pressure Stall Information](https://docs.kernel.org/accounting/psi.html).

### Passive MMC wear observations

`eld-engine storage mmc-health --card mmc0:0001` reads the named
MMC card's cached kernel sysfs `pre_eol_info` and `life_time` when
the MMC driver exposes them. The integers are raw device codes. They
are not fresh readings, exact wear percentages, a health verdict, or
estimated time to failure. Linux only populates those cached fields
for supported Extended CSD revisions (revision 7 or later), so a zero
can also be an unpopulated value on an older card. The tool preserves
zero as a raw observation without interpreting it as healthy. SD cards
are not supported by this command. No block device is opened, no MMC
command is sent, and no serial number is collected. See the Linux
[MMC attribute definitions](https://github.com/torvalds/linux/blob/master/drivers/mmc/core/mmc.c).

### Preview a qualification profile

```sh
eld-engine qualify plan --profile bring-up
eld-engine qualify plan --profile overnight --include-local-load
```

The versioned JSON plan does not detect hardware, write reports or run any
step. Bring-up is always passive. Quick, manufacturing and overnight preview
one CPU and one RAM phase only when requested with `--include-local-load`;
otherwise those steps are marked skipped. Run selected steps separately on a
suitable test machine. Overnight plans two sequential four-hour targets, not
a guaranteed wall-clock runtime or a pass/fail verdict. Board-specific
checks require an explicit profile. Physical peripherals, a network peer,
and storage writes always require separately selected fixtures or budgets
and are never planned for automatic execution.


With `--include-local-load`, `--local-test cpu|ram` limits the preview
to one workload; otherwise both are shown. Add `--max-local-load-secs`
(1 to 1,209,600 seconds) to reject a plan whose proposed sequential load
targets exceed that budget. The budget does not include setup/cleanup,
does not change the planned workload durations, and does not start or
enforce a running stress test.

For bounded bring-up observations, use `eld-engine qualify passive` or
`eld-engine qualify passive --board-profile board.profile`. This reads one
inventory snapshot, with no timed sampling, stress, or external network.
It writes no files by default. The versioned result wraps the existing inventory report.
Without a profile it reports the board check as not configured. For a
supplied profile read `inventory.board_check` for the scoped PCI result;
an exit code of zero means the observation was emitted, not that the board
passed. This command does not run the quick, manufacturing, or overnight
workloads, or issue an overall qualification verdict. You can also add
`--network-interface eth0` to embed a single read-only NIC observation.
It is not sampled twice, sends no packets, and is independent of the PCIe
board check. If the interface is absent, its observation says unavailable;
that does not imply a qualification failure. Add `--nvme-controller nvme0`
to also include one read-only controller metadata observation. This does not
collect or decode a SMART capture, touch a storage device node, or run a
storage test. A missing controller is reported without changing board
assessment.

For a machine-readable handoff, add `--evidence-json NEW_PATH`. It writes
an owner-only JSON file up to 1 MiB with the exact stdout observation and
wall-clock start/end plus monotonic elapsed milliseconds. It refuses to
overwrite any existing path. Treat the file as complete only after the
command exits successfully and the entire JSON validates; interrupted writes
may leave an incomplete file. The timestamps describe the collection host,
not a trusted external clock or a qualification verdict.

For correlation across machines, `--run-id HEX32` echoes an explicitly
chosen 32-character lowercase hexadecimal token in the observation, including
the observation embedded in the file. Create a fresh random token for each
invocation if you need to distinguish reruns. The identifier is not a path,
and does not by itself authenticate the sender, device, or observed data.
It is omitted if not supplied.


### PCI accelerator candidates

`eld-engine accelerator inventory` lists PCI display-controller and
processing-accelerator class matches with BDF and PCI vendor/device IDs.
It reads kernel PCI metadata only: the driver and optional module name are
observed via kernel symlinks only. No vendor CLI, runtime context, device
node or workload is opened. A bound driver does not imply health or
userspace runtime availability, and a missing module link can be normal. Display classes do not establish GPU
compute capability. An empty result does not exclude SoC-integrated or
differently classified accelerators. Missing PCI sysfs and a partial scan
are reported separately from a completed scan with no class matches.
See [Linux PCI sysfs](https://docs.kernel.org/PCI/sysfs-pci.html).


`eld-engine accelerator interfaces` reports DRM render and Linux compute
accelerator registrations separately. Missing class and readable empty class
are distinct. A reported PCI association requires the class entry’s
canonical device parent to match an indexed PCI function. Platform and
ambiguous parent registrations can still appear with a null PCI function.
This reads metadata only, not a character device, and cannot establish
userspace access, compute capability or health.
[Linux accelerator ABI](https://github.com/torvalds/linux/blob/v6.6/Documentation/accel/introduction.rst#L59-L81)


On a known AMD GPU, `eld-engine accelerator amdgpu-hwmon --device BDF`
reads documented temperature and SoC power channels from the selected PCI
function only if it is bound to AMDGPU. Power is not GPU-only on APUs.
Values are exact decimal strings; missing values are not zero. Reads may
wake a power-managed device. The opt-in command does not run automatically,
set power or fan controls, load drivers, or run compute.
[Linux AMDGPU hwmon reference](https://docs.kernel.org/gpu/amdgpu/thermal.html#hwmon-interfaces)


### Selected INA3221 power-rail readings

`eld-engine power ina3221 --node 1-0040` reads the rail label, bus voltage
(mV), and current (mA) of a selected INA3221 kernel device, where present
on Jetson Orin and other supported boards. Use your board’s actual bus and
address; there is no probing across buses or hard-coded rail mapping.
Readings are sequential, not simultaneous. The tool does not estimate
power, set current limits, or contact a raw I2C device. Missing metrics
remain unavailable, not zero.
[Kernel driver ABI](https://docs.kernel.org/hwmon/ina3221.html)

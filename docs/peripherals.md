# Peripheral bus and input fixtures

## Peripherals

Active peripheral benchmarks run only when requested directly. Automatic stress only observes allowlisted PCIe, USB and HDMI status attributes in sysfs; it never opens a peripheral device node or initiates an external test.

## UART loopback

Use a dedicated fixture with the selected port's TX connected to its RX at the voltage and signaling levels specified for that port. Across separate connectors, share a ground and use compatible levels. Keep the boot console, login terminals, and any attached equipment out of the fixture. Use TX/RX only for the loopback. RS-232 signaling requires a suitable transceiver before it can connect to a logic-level UART.

```sh
./target/release/eld-engine uart --device /dev/ttyUSB0 --loopback --bytes 4096
```

`--loopback` acknowledges the dedicated fixture. The command checks the Linux console records, rejects standard streams and the controlling terminal, and accepts recognized `/dev` serial tty nodes such as `ttyUSB`, `ttyS`, and Jetson `ttyTHS` nodes, their aliases, or `/dev/pts/N` for testing. It requires the standard tty line discipline and a nonzero existing baud rate. The benchmark uses 115200 baud, 8 data bits, no parity, one stop bit, and no flow control. `--bytes` accepts 1–65536 and defaults to 4096. `--timeout-ms` accepts 1–30000 and defaults to 10000.

The result is verified payload bytes per second over the loopback, including both userspace I/O and comparison. The command opens the device nonblocking, leaves the original hang-up-on-close flag (`HUPCL`) intact, and restores the original tty settings on success or a handled error. A driver can still change modem lines when opening a port or when closing the last descriptor, depending on its existing settings. Use a fixture that does not rely on modem lines. Forced termination can interrupt settings restoration. The deadline cannot preempt a kernel driver call that stalls.

The UART implementation uses the Linux tty ABI for x86, x86-64, Arm, AArch64, and MIPS32 targets with glibc or musl. Other Linux targets return an explicit UART error before accessing a device. [Linux device numbers](https://docs.kernel.org/admin-guide/devices.html) · [Generic kernel termios ABI](https://github.com/torvalds/linux/blob/v6.12/include/uapi/asm-generic/termbits.h) · [MIPS kernel termios ABI](https://github.com/torvalds/linux/blob/v6.12/arch/mips/include/uapi/asm/termbits.h)

## I2C fixture read latency

Use a dedicated adapter and a fixture documented to support standalone reads of the selected length at the selected address. Reads can have side effects, including consuming queued data, clearing status, or advancing a device pointer. Choose an isolated fixture without equipment that relies on that device state.

```sh
./target/release/eld-engine i2c --adapter /dev/i2c-1 --address 0x48 \
  --fixture --read-bytes 2 --samples 100
```

The adapter must expose Linux `i2c-dev` with raw I2C support. Enter one unreserved 7-bit address from `0x08` through `0x77` in decimal or hexadecimal. The command uses the kernel's ordinary address selection and respects a busy address. Each sample issues exactly one fixed-length read. The command has no address scan, forced selection, write transaction, or register selection. It reports received bytes and mean userspace read latency. `--read-bytes` accepts 1–32 and defaults to 1. `--samples` accepts 1–1024 and defaults to 32. [Linux I2C userspace interface](https://docs.kernel.org/i2c/dev-interface.html)

## SPI fixture throughput

Connect the controller's MOSI to its MISO on a dedicated loopback fixture with compatible signal levels. Keep other peripherals disconnected from that bus. The controller can toggle clock and chip select during the test. Use an existing Linux `spidev` node with an 8-bit, single-lane full-duplex configuration.

```sh
./target/release/eld-engine spi --device /dev/spidev0.0 --loopback --bytes 4096
```

The command uses the existing SPI mode and transfer settings and reports the configured maximum clock. It does not write mode, word size, or speed settings. Each full-duplex transfer is at most 1024 bytes, and the result counts only received bytes that match the sent payload. `--bytes` accepts 1–65536 and defaults to 4096. The reported userspace rate includes transfers and verification. [Linux spidev interface](https://docs.kernel.org/spi/spidev.html)

## GPIO input read latency (legacy sysfs)

Use the Linux sysfs GPIO number of a line already exported and configured as an input. The example assumes an existing `gpio23` input. Sysfs GPIO is a legacy interface and may be missing from the target kernel. The benchmark requires a readable `direction` attribute equal to `in` and a readable `value` attribute. It opens both read-only and leaves export, pin direction, and pin state under the board's existing configuration. Keep the line exported as an input throughout the run. [Linux sysfs GPIO ABI](https://docs.kernel.org/userspace-api/gpio/sysfs.html)

```sh
./target/release/eld-engine gpio --pin 23 --samples 1000
```

`--samples` accepts 1–100000 and defaults to 1000. `--timeout-ms` accepts 1–30000 and defaults to 5000. The command reports the mean time for userspace to rewind and read one value, along with the number of high samples. Electrical propagation and edge timing require separate instrumentation. The deadline is checked between samples and cannot interrupt a stalled kernel GPIO read.

## GPIO input read latency (chardev v2)

Select an available spare input by its character-device chip and chip-relative line offset. The command checks the kernel-reported direction, requests the line exclusively as-is, then checks the direction again before reading. It leaves direction, bias, and logical polarity as configured, respects a line already in use, and releases the line descriptor at exit. The result reports userspace value-read latency and counts logical active values. The kernel controls the line's default state after release. [Linux GPIO v2 line requests](https://docs.kernel.org/userspace-api/gpio/gpio-v2-get-line-ioctl.html)

```sh
./target/release/eld-engine gpio --chip /dev/gpiochip0 --offset 7 \
  --input-fixture --samples 1000
```

The chardev mode uses the same sample limits as legacy GPIO. It requires the GPIO v2 ABI, which entered mainline in Linux 5.10 and depends on the target kernel configuration. A legacy-only kernel can use `--pin` if the sysfs input exists. The two GPIO modes require separate invocations. [Linux GPIO chardev API](https://docs.kernel.org/userspace-api/gpio/chardev.html)

I2C, SPI, and GPIO chardev accept `--timeout-ms` from 1–30000 with a 5000 ms default. Each run is foreground and checks its monotonic deadline between operations. A blocked kernel driver call can delay that check. These three commands require sysfs device identity records and the Linux device ABI on x86, x86-64, Arm, AArch64, or MIPS32 with glibc or musl. Other ABIs fail before accessing the device.

## IIO raw input cadence

```sh
./target/release/eld-engine iio --device iio:device0 --channel in_voltage0 --samples 100
```

Select an available input channel by its sysfs stem. This example reads `in_voltage0_raw` as a signed integer. Use a spare input where repeated reads are appropriate. The command rewinds and reads the same attribute for every sample. It reports the raw range and mean, mean userspace seek-plus-read time, and cadence and standard deviation between successive read completions.

`--samples` accepts 2–10000 with a default of 100. `--timeout-ms` accepts 1–30000 with a default of 5000. The command checks the deadline between reads. A stalled driver call can extend the run. These observations use userspace monotonic time. Buffered hardware timestamps, converter sample rates and calibrated units require device-specific information. The benchmark performs read-only access to the selected raw attribute. [Linux IIO sysfs ABI](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-bus-iio)

PCIe and IIO validation used inert sysfs fixtures and CLI rejection tests. OpenWrt 25.12.6 AArch64 and 24.10.8 MIPS little-endian SDK packages built successfully. CLI rejection passed in those userspaces and in the Jetson Linux 39.2.2 sample userspace under QEMU user mode.

## USB bulk loopback

Use an isolated USB fixture with firmware that echoes each bulk OUT transfer onto its bulk IN endpoint. Select the device node, current interface number, and both endpoint addresses explicitly. The interface must be unbound from kernel drivers and expose both bulk endpoints in its current configuration and alternate setting.

```sh
./target/release/eld-engine usb --device /dev/bus/usb/001/011 \
  --interface 0 --out-endpoint 0x01 --in-endpoint 0x81 --loopback --bytes 4096
```

The command claims the selected interface, uses bounded usbfs bulk OUT and IN transfers of at most 4096 bytes each, verifies the echo, and releases the interface. It exposes no control messages, resets, configuration changes, or driver detachment. `--bytes` accepts 1–65536 (default 4096). `--timeout-ms` accepts 1–30000 (default 5000). Each request uses the remaining run time as its nonzero kernel timeout. The result includes both directions and verification. [Linux USB userspace API](https://docs.kernel.org/driver-api/usb/usb.html)

USB validation used an inert sysfs interface fixture and mocked transfers, including short sends, corrupted replies, and a deadline. Native and SDK ABI checks covered 64-bit, Armv7, and both MIPS byte orders. Package install, peer TCP, USB and CAN rejection, and package removal passed in disposable OpenWrt 21.02.7 AArch64, 25.12.6 AArch64, and 24.10.8 MIPS little-endian guests. USB and CAN fixture unit tests also passed under Jetson Linux 39.2.2 QEMU user mode. Real USB transfers remain unverified.

## Classic CAN and CAN FD peer fixture

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

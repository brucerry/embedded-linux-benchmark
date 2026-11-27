# Diagnostic guides by component

The former benchmark guide is split into the guides below. The README lists
board-side commands. Use `cargo build --release --locked` and
`./target/release/eld-engine --help` to inspect the CLI.

## [System inventory, CPU, memory and pressure](system.md)

- <a name="bring-up-inventory"></a>[Bring-up inventory](system.md#bring-up-inventory)
- <a name="cpu-and-ram"></a>[CPU and RAM](system.md#cpu-and-ram)
- <a name="read-host-resource-inventory"></a>[Read host resource inventory](system.md#read-host-resource-inventory)
- <a name="save-an-inventory-evidence-file"></a>[Save an inventory evidence file](system.md#save-an-inventory-evidence-file)
- <a name="passive-linux-pressure-snapshot"></a>[Passive Linux pressure snapshot](system.md#passive-linux-pressure-snapshot)

## [PCIe device and link diagnostics](pcie.md)

- <a name="pcie-link-information"></a>[PCIe link information](pcie.md#pcie-link-information)
- <a name="check-expected-pci-devices"></a>[Check expected PCI devices](pcie.md#check-expected-pci-devices)
- <a name="reuse-a-board-profile"></a>[Reuse a board profile](pcie.md#reuse-a-board-profile)
- <a name="pcie-health-snapshot"></a>[PCIe health snapshot](pcie.md#pcie-health-snapshot)

## [GPU and accelerator diagnostics](gpu.md)

- <a name="pci-accelerator-candidates"></a>[PCI accelerator candidates](gpu.md#pci-accelerator-candidates)

## [Board power-rail observations](power.md)

- <a name="selected-ina3221-power-rail-readings"></a>[Selected INA3221 power-rail readings](power.md#selected-ina3221-power-rail-readings)

## [Storage benchmarks and health](storage.md)

- <a name="storage"></a>[Storage](storage.md#storage)
- <a name="passive-nvme-controller-status"></a>[Passive NVMe controller status](storage.md#passive-nvme-controller-status)
- <a name="decode-a-previously-captured-smart-log"></a>[Decode a previously captured SMART log](storage.md#decode-a-previously-captured-smart-log)
- <a name="passive-mmc-wear-observations"></a>[Passive MMC wear observations](storage.md#passive-mmc-wear-observations)

## [Network link diagnostics and lab tests](network.md)

- <a name="passive-network-link-health"></a>[Passive network link health](network.md#passive-network-link-health)
- <a name="tcp-loopback"></a>[TCP loopback](network.md#tcp-loopback)
- <a name="tcp-peer-throughput-and-round-trip-latency"></a>[TCP peer throughput and round-trip latency](network.md#tcp-peer-throughput-and-round-trip-latency)

## [Peripheral bus and input fixtures](peripherals.md)

- <a name="peripherals"></a>[Peripherals](peripherals.md#peripherals)
- <a name="uart-loopback"></a>[UART loopback](peripherals.md#uart-loopback)
- <a name="i2c-fixture-read-latency"></a>[I2C fixture read latency](peripherals.md#i2c-fixture-read-latency)
- <a name="spi-fixture-throughput"></a>[SPI fixture throughput](peripherals.md#spi-fixture-throughput)
- <a name="gpio-input-read-latency-legacy-sysfs"></a>[GPIO input read latency (legacy sysfs)](peripherals.md#gpio-input-read-latency-legacy-sysfs)
- <a name="gpio-input-read-latency-chardev-v2"></a>[GPIO input read latency (chardev v2)](peripherals.md#gpio-input-read-latency-chardev-v2)
- <a name="iio-raw-input-cadence"></a>[IIO raw input cadence](peripherals.md#iio-raw-input-cadence)
- <a name="usb-bulk-loopback"></a>[USB bulk loopback](peripherals.md#usb-bulk-loopback)
- <a name="classic-can-and-can-fd-peer-fixture"></a>[Classic CAN and CAN FD peer fixture](peripherals.md#classic-can-and-can-fd-peer-fixture)

## [Camera and HDMI diagnostics](display.md)

- <a name="camera-delivery-rate"></a>[Camera delivery rate](display.md#camera-delivery-rate)
- <a name="hdmi-connector-observations-and-capture-delivery"></a>[HDMI connector observations and capture delivery](display.md#hdmi-connector-observations-and-capture-delivery)

## [Stress tests and qualification profiles](stress.md)

- <a name="automatic-stress"></a>[Automatic stress](stress.md#automatic-stress)
- <a name="elapsed-time-runs"></a>[Elapsed-time runs](stress.md#elapsed-time-runs)
- <a name="optional-final-json-summary"></a>[Optional final JSON summary](stress.md#optional-final-json-summary)
- <a name="preview-a-qualification-profile"></a>[Preview a qualification profile](stress.md#preview-a-qualification-profile)

## [Validation and hardware coverage](validation.md)

- <a name="validation"></a>[Validation](validation.md#validation)
- <a name="i2c-spi-and-gpio-chardev-verification"></a>[I2C, SPI, and GPIO chardev verification](validation.md#i2c-spi-and-gpio-chardev-verification)

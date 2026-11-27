# GPU and accelerator diagnostics

Start with the built-in commands on the board:

```sh
eld-engine accelerator inventory
eld-engine accelerator interfaces
# Replace this example with the PCI address (BDF) from inventory.
GPU=0000:65:00.0
eld-engine pcie link --device "$GPU"
eld-engine pcie health --device "$GPU" --sample-ms 1000
```

Check whether the device enumerated, which driver bound to it, and whether a
DRM render or Linux compute interface registered. Compare the negotiated PCIe
link to the expected speed and width. The health command records available AER
counters and only compares counters with matching device identity. It does
not measure PCIe throughput. An integrated accelerator may not have a PCI
address; a bound driver or registered interface does not prove compute works.

See [accelerator inventory and interfaces](benchmarks.md#pci-accelerator-candidates)
and [PCIe health](benchmarks.md#pcie-health-snapshot) for limits and output details.

## AMD GPU sensors

For a device bound to AMDGPU:

```sh
eld-engine accelerator amdgpu-hwmon --device "$GPU"
```

The command reads selected temperature and power channels when the driver
exposes them. APU power may cover the whole SoC, and sensor reads may wake a
power-managed device. Missing readings are not zero. This command does not
set controls or collect live ECC/error counters.

## NVIDIA and Jetson

The generic inventory and PCIe commands above work where Linux exposes
the relevant interfaces. The built-in CLI does not query live NVIDIA ECC
or compute performance. On supported Jetson boards, the
[INA3221 command](benchmarks.md#selected-ina3221-power-rail-readings) reads
voltage and current from a selected power rail. Check the board's rail
mapping before attributing a measurement to the GPU.

For compute result verification and host-to-device/device-to-host transfers,
use the [optional CUDA helper](cuda.md), a separate `eld-engine-cuda` executable
built with the NVIDIA Toolkit. The helper's build and CLI checks passed;
physical GPU performance remains unverified.

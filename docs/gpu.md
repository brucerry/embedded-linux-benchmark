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

See [accelerator inventory and interfaces](#pci-accelerator-candidates)
and [PCIe health](pcie.md#pcie-health-snapshot) for limits and output details.

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
[INA3221 command](power.md#selected-ina3221-power-rail-readings) reads
voltage and current from a selected power rail. Check the board's rail
mapping before attributing a measurement to the GPU.

For compute result verification and host-to-device/device-to-host transfers,
use the [optional CUDA helper](cuda.md), a separate `eld-engine-cuda` executable
built with the NVIDIA Toolkit. The helper's build and CLI checks passed;
physical GPU performance remains unverified.

## Optional NVIDIA health capture

This separate helper is not part of the built-in executable or the default
suite. On Linux, build it with NVIDIA's official `nvml.h` and the appropriate
development library. The header and installed runtime must support
`nvmlDeviceGetTemperatureV`. Use the actual driver library on the target,
not a toolkit stub.

```sh
mkdir -p target
cc -std=c11 -O2 -I"$NVML_INCLUDE_DIR" cuda/nvml-health.c \
  -L"$NVML_LIB_DIR" -lnvidia-ml -o target/eld-engine-nvml
GPU=0000:01:00.0
if capture=$(target/eld-engine-nvml --device "$GPU"); then
  eld-engine accelerator nvidia-health-decode --device "$GPU" --capture-text "$capture"
fi
```

Set the include and library paths to the NVIDIA development files. Choose
the BDF from accelerator inventory. The helper requests NVML's no-attach
initialization mode and queries the selected GPU's identity, temperature,
power, and volatile corrected and uncorrected ECC totals. It does not
enumerate devices, start compute, reset counters, or set controls.
Vendor queries may still affect device power state. Unsupported,
inaccessible, and unavailable readings stay separate from zero.
The unsigned capture format reports negative temperatures as unavailable.

This helper was checked against the official header with synthetic library
responses only. Physical NVIDIA and Jetson runtime behavior and telemetry
have not been verified. Run the fixture without a driver or GPU:

```sh
python3 tests/nvml_health_fixture.py --include-dir "$NVML_INCLUDE_DIR"
```

## PCI accelerator candidates

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

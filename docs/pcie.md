# PCIe device and link diagnostics

## PCIe link information

```sh
./target/release/eld-engine pcie link --device 0000:01:00.0
```

Use the complete PCI domain, bus, device and function for the device you intend to observe. The command reads the kernel's negotiated and maximum link speed and lane width for that device. Missing attributes are reported as unavailable. The speeds are link signaling rates in GT/s, and the maximum values are capabilities. Measuring payload throughput requires a separate test. [Linux PCI sysfs implementation](https://github.com/torvalds/linux/blob/v6.12/drivers/pci/pci-sysfs.c)

`pcie throughput --device BDF --fixture` returns an unavailable error before opening a device. The published stock endpoint-test read, write and copy requests specify a transfer size and flags. The reviewed Linux 6.12 and 6.18 host drivers wait for IRQ completion without a transfer timeout. They perform completion or integrity checks but expose no per-direction timing or completed byte counter. A userspace timeout cannot interrupt that wait. A bounded throughput benchmark requires an identifiable driver ABI with a completion deadline and an exact dedicated endpoint fixture. [Linux 6.12 endpoint-test UAPI](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/pcitest.h) · [Linux 6.18 endpoint-test driver](https://github.com/torvalds/linux/blob/v6.18/drivers/misc/pci_endpoint_test.c)

## Check expected PCI devices

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

## Reuse a board profile

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

## PCIe health snapshot

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

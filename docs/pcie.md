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

Use `--sample-ms 1000` to take a second snapshot after a bounded wait (1–60000 ms). No load is generated. The JSON contains both snapshots, the measured interval between sample starts, and a per-observer delta only when boot and sysfs identities match and both driver bindings are stable and agree. This does not detect an unobserved rebind that returns to the same driver between samples. A decrease, a changed category set, or an unavailable counter never becomes an invented zero. Counter deltas show what changed during the interval, not which component caused it.

To assess a board's expected link, add `--min-width 4 --min-speed-gtps 8.0`.
The optional `target_link_expectation` object compares only the selected device's
negotiated values. It reports `below_minimum`, `meets_minimum`, or an observation
state such as `unavailable`. Device maximum capabilities are not used as expected
board wiring. A shortfall is evidence for investigation, and does not on its own
change the command's exit status.

### Compare two saved observations offline

Use the same trusted host and device for both captures. Create a private
directory and allow at least one uptime clock tick between captures.

```sh
umask 077
eld-engine pcie health --device 0000:01:00.0 --save-snapshot > before.json
sleep 1
eld-engine pcie health --device 0000:01:00.0 --save-snapshot > after.json
python3 scripts/compare_pcie_snapshots.py before.json after.json
```

The comparison reports the SHA-256 of each supplied file. When a controller
already has trusted capture digests, pass both
`--before-sha256 BEFORE_SHA256 --after-sha256 AFTER_SHA256` to reject
changed or swapped inputs. With no expected digests, the input selection is
`unbound`. Matching the selected bytes does not authenticate the hardware.

Run the examples inside a private directory; do not post the raw snapshots.
Only the opt-in format includes the kernel boot ID and sysfs device/inode
numbers. The comparison omits them and does not access hardware. The
negotiated and maximum link fields must match the snapshot schema, but this
command calculates AER counter differences, not link or throughput trends.
It exits 0
when records can be compared (individual fields may still be unavailable),
2 for incompatible continuity evidence, and 1 for invalid input. It rejects
different boots, overlapping or reversed capture windows, changed observers,
and changed or indeterminate driver bindings. A counter decrease or category
change invalidates that field instead of creating a zero. Matching IDs are a
necessary continuity check, not hardware authentication or proof that no reset
or same-driver rebind happened between captures. A different binding changes
the observation conditions; it does not itself establish a counter reset.
Keep sampling intervals short.

A successful comparison can still have no usable counter evidence. Check
`counter_coverage`: `unavailable` means all fields were unavailable,
`not_comparable` means none were observed and at least one was inaccessible,
invalid, or changed state, and `partial` or `available` require actual
observations. An observed `"0"` is a measured zero increment. It is not
interchangeable with an unavailable counter, and neither certifies link health.
The field totals count observable fields, not error events.

### Saved PCIe AER notices

On the host, pass a saved plain UTF-8 kernel log and an independently known
root-port BDF:

```sh
python3 -B scripts/check_pcie_aer_log.py --log boot.log --observer-bdf 0000:00:01.0
```

This extracts only recognized root-port AER notice lines, with their line
numbers, observer BDF, printed source ID and severity. It does not forward
unrelated log text, read the live kernel, or infer a failing endpoint from
the source ID. Linux can treat a reported bus-00 source ID as unreliable
and consult device status instead. The checker preserves the printed source
ID without resolving it to an endpoint. Bus 00 can still be valid in an
ordinary PCI inventory. [Linux AER source selection](https://github.com/torvalds/linux/blob/v6.6/drivers/pci/pcie/aer.c#L772-L803)
The number of log notices is not a hardware error count.
No matches does not mean zero errors, and unrelated vendor log formats are
not included. An optional `--expected-sha256` pins the saved bytes.
[Linux AER notice format](https://github.com/torvalds/linux/blob/v6.6/drivers/pci/pcie/aer.c#L690-L698)


## Selected PCIe function locality

```sh
eld-engine pcie locality --device 0000:01:00.0
```

Choose a known PCI address. This reads the selected function’s Linux
`numa_node` and `local_cpulist` only. A reported NUMA node of `-1` is
unknown, not node zero. The CPU list is a kernel topology hint, not a
measurement or the process’s allowed CPU mask. The command does not bind
tasks, configure IRQs, benchmark the link, or modify sysfs.

To verify saved stdout offline against the explicitly selected PCI function:

```sh
python3 -B scripts/check_pcie_locality_evidence.py \
  --report locality.json --device 0000:01:00.0
```

Optionally pass the independently selected `--expected-sha256`. A matching
report does not verify hardware identity or infer CPU affinity.

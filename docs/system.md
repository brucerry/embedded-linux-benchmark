# System inventory, CPU, memory and pressure

## Bring-up inventory

`eld-engine inventory` prints a read-only JSON summary of process architecture,
Linux kernel release, distribution, and model or firmware fields exposed by
Device Tree or DMI. It does not scan the network, read machine IDs or serial
numbers, or execute a system configuration file. The architecture describes
the executable, not a claim about the physical board under emulation.
Unavailable, inaccessible, and invalid observations stay separate.
The reported `distribution_build_id` is the optional `BUILD_ID` label from
`os-release`, separate from `VERSION_ID`, the kernel release, and any DMI
BIOS version. On OpenWrt it can distinguish builds with the same release
label. It is neither an image digest nor a signature. Older evidence
without the label remains comparable for the other fields.

## CPU and RAM

```sh
eld-engine cpu --duration-ms 500
eld-engine ram --duration-ms 500 --buffer-kib 1024
```

The CPU test measures single-threaded 32-bit integer throughput. The RAM test
copies between two buffers of the selected size. Both are active local loads:
use short runs on a lab board and interpret results in the context of its power,
cooling and firmware configuration. See [automatic stress](stress.md#automatic-stress) for
a longer or combined run.

## Read host resource inventory

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

## Save an inventory evidence file

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

### Read one hwmon temperature channel

```sh
eld-engine hwmon temperature --sensor hwmon0 --channel 1
```

Select a known Linux hwmon index and channel for the board under test. This
reads only its `temp1_input` (or the selected channel up to 32) and reports
a signed raw value with unknown unit. Some thermistor/ADC drivers expose
voltage rather than Celsius in temperature-named files. Do not convert the
reading or use it against a temperature threshold without independently
checking the driver and channel. [Linux hwmon ABI](https://docs.kernel.org/6.6/hwmon/sysfs-interface.html)
The command does not enumerate sensors or write thresholds.
A sensor index is local to a boot and is not a hardware identifier. An absent,
inaccessible, invalid or replaced observation is not an observed temperature.
Use an explicit sensor from the platform documentation or local sysfs.

To validate a saved stdout capture offline on the controller:

```sh
python3 -B scripts/check_hwmon_temperature_evidence.py \
  --report sensor.json --sensor hwmon0 --channel 1
```

Use `--expected-sha256` if the controller has independently selected the
capture digest. A verified report records what the device reported; it
does not verify the unit, sensor calibration, or hardware identity. Older
development captures carrying a Celsius label are interpreted as raw values.

### Inspect one saved thermal zone

```sh
eld-engine inventory --thermal-sample-ms 1000 --evidence-json ./thermal.json
python3 -B scripts/check_inventory_thermal.py --inventory thermal.json --zone thermal_zone0
```

Choose a zone reported for that device. This offline check validates the two
snapshots and their reported arithmetic. It distinguishes a zone that wasn't
reported from a reported one with no comparable value. It never discovers a
sensor, starts a workload, or treats matching labels as proof of the same
physical sensor across runs. Use `--expected-inventory-sha256` to match an
independently selected report digest.

### Compare saved inventories

```sh
python3 scripts/compare_inventory_evidence.py --before baseline.json --after after-update.json
```

This offline comparison reports changes in observed kernel, distribution,
model and DMI firmware labels. A field missing in either capture is
`not_comparable`, even when both captures report it unavailable. Use
`--expected-before-sha256` and `--expected-after-sha256` together if
independent expected digests are available. Capture order is supplied by
the caller, not inferred from the device clocks. The tool does not claim
the files describe the same board, establish boot continuity, or compare
dynamic readings such as free memory or temperature.

### Check explicit platform expectations

Supply an independently selected JSON profile for the board or image under test:

```json
{"schema_version":1,"expected":{"kernel_release":"6.6.1","dmi_firmware_version":"v1.2"}}
```

```sh
python3 scripts/check_inventory_profile.py --inventory after-update.json --profile expected.json
```

Use exact reported labels (not patterns) and only select fields your design
actually exposes. The result is `match` when every selected label matches,
`mismatch` when at least one observed label differs, and `incomplete` when
no observed label differs but at least one cannot be compared. A successfully
parsed evaluation exits zero for all three outcomes; consumers must check
the JSON status. This is not a firmware measurement, a board identity check,
or a hardware qualification.

## Read one hwmon fan channel

```sh
eld-engine hwmon fan --sensor hwmon0 --channel 1
```

Choose the device's current hwmon index and fan channel explicitly. The
command reads just `fanN_input` and reports RPM as an exact decimal string
when observed. Zero RPM is an observation, not a cooling failure verdict;
an absent tachometer reports unavailable. No PWM, fan control, or load is
applied. Indices can change after reboot.
[Linux hwmon fan ABI](https://docs.kernel.org/hwmon/sysfs-interface.html#fans)

Review saved output offline with the selected sensor and channel:

```sh
python3 -B scripts/check_hwmon_fan_evidence.py --report fan.json --sensor hwmon0 --channel 1
```

An optional independently recorded SHA-256 binds the exact report bytes.
This does not authenticate the device or assess cooling health.

## Read one thermal cooling device

```sh
eld-engine thermal cooling --device cooling_device0
```

Select a current cooling-device index. This reads its type, current state,
and maximum state from Linux thermal sysfs. The states are driver-defined
levels, not degrees, RPM, or throughput. A nonzero state does not prove
throttling or a hardware fault; a zero state does not prove the device is
cool. Fields are read separately; missing remains distinct from zero.
The command never writes a cooling state.
[Linux thermal sysfs ABI](https://docs.kernel.org/driver-api/thermal/sysfs-api.html)

To review saved output without the target:

```sh
python3 -B scripts/check_thermal_cooling_evidence.py --report cooling.json --device cooling_device0
```

When available, pass `--expected-sha256` from the trusted capture record.
Validation does not infer throttling, eligibility for stress, or identity.

## Read CPU thermal throttle events

```sh
eld-engine thermal throttle --cpu 0
```

Reads two Linux sysfs event counters for one explicitly selected logical
CPU, where the kernel exposes the Intel thermal-throttle interface.
The count is cumulative since kernel initialization, not a live
throttling flag or temperature. Do not sum package counts across CPUs or
compute a delta across boots. Unsupported targets report unavailable,
not zero. The command does not read MSRs, reset counters, or start load.
[Linux Intel thermal throttle interface](https://docs.kernel.org/admin-guide/thermal/intel_thermal_throttle.html)

To validate one saved snapshot without the device:

```sh
python3 -B scripts/check_cpu_throttle_evidence.py --report throttle.json --cpu 0
```

For an exact byte match, use the independently recorded
`--expected-sha256`. The saved report does not establish boot continuity or
package membership. The checker does not calculate deltas or totals for
several logical CPUs.

## Read one CPU frequency policy

```sh
eld-engine cpufreq policy --policy policy0
```

Unlike the broader inventory, this reads only the selected policy and
includes its minimum scaling limit. All frequencies are in kHz.
`scaling_cur_freq` commonly reflects the last setting requested by the
driver, not a hardware measurement. `cpuinfo_cur_freq` is reported
separately when the kernel exposes it; no fallback substitutes one for
the other. Fields are read independently and do not diagnose throttling.
No governor, frequency or policy settings are changed.
[Linux CPU frequency guidance](https://docs.kernel.org/admin-guide/pm/cpufreq.html)

Validate saved output without the target with
`python3 -B scripts/check_cpufreq_policy_evidence.py --report frequency.json --policy policy0`.
An independently recorded `--expected-sha256` can select exact bytes.
The checker does not infer hardware frequency from driver output or assess
thermal throttling from independent samples.

## Read a selected HugeTLB pool

```sh
eld-engine memory hugepages --size-kib 2048
```

Select a page size supported by the target. The per-size sysfs
`nr_hugepages` read is the **total** pool, including surplus pages. It is
not the persistent-pool setting read from `/proc/sys/vm/nr_hugepages`.
For example, total 8 and surplus 3 describe 5 persistent pages if sampled
at the same instant; do not add surplus to total. `free_hugepages` can
include pages committed to reservations, so neither that count nor an
inferred unreserved count guarantees a process can allocate pages.

Counts are pages, not bytes. The four values are read independently, not
as an atomic snapshot, so the checker preserves each observation without
deriving counts or enforcing arithmetic across them. Missing is not zero.
The command does not allocate memory, change kernel limits, or inspect
transparent huge pages.
[Linux HugeTLB pools](https://docs.kernel.org/admin-guide/mm/hugetlbpage.html) ·
[Linux 6.6 sysfs read implementation](https://github.com/torvalds/linux/blob/v6.6/mm/hugetlb.c#L3490-L3503)

To validate saved output on the analysis machine, use the same selected
size and optionally a separately recorded digest:

```sh
python3 scripts/check_hugetlb_evidence.py --report hugetlb.json --size-kib 2048
```

This checks the report structure and preserves observed zero, unavailable
counters, and an unavailable pool as distinct states. It does not verify
device identity or allocation capacity.

## Passive Linux pressure snapshot

`eld-engine pressure` reads the CPU, memory and I/O files under
`/proc/pressure` once, without starting a workload or registering a kernel
trigger. The JSON preserves the three percentage windows as decimal strings
and exact cumulative stall time in microseconds. Missing resources are
separate from observed zero. System CPU `full` is not applicable even if
the kernel exposes its backward-compatibility zero. This is system-level
pressure, not a per-process measurement or a benchmark result.
See [Linux Pressure Stall Information](https://docs.kernel.org/accounting/psi.html).

### Bounded passive pressure series

Use an isolated run ID and a fresh evidence path:

```sh
eld-engine qualify passive --include-system-health \
  --pressure-series-samples 2 --pressure-series-gap-ms 100 \
  --run-id 0123456789abcdeffedcba9876543210 \
  --evidence-json /tmp/eld-pressure.json
```

The count is 2–8 and the requested pause between samples is 100–1000 ms.
Pressure is system-wide kernel telemetry. The command adds waits and reads,
not a workload, and cannot establish which process caused the stalls.
A reported zero is different from unavailable telemetry; system CPU
`full` is not applicable.

For Hub handoff, pass the two pressure options and `--feature system-health`
to `scripts/plan_hub_passive.py` with captured capabilities, the chosen run ID,
and the evidence path. The preview does not execute the command. Keep the
plan, stdout, and evidence together: `scripts/check_hub_evidence.py --plan`
checks that the count and gap match the approved plan, and validates the
values without interpreting them as a health or performance rating.

## Selected memory-controller error trends

```sh
eld-engine ram edac --controller mc0
eld-engine ram edac --controller mc0 --sample-ms 1000
```

The first command reads only the selected controller's Linux EDAC counter
attributes. The second waits 1–2000 ms and takes another observation in
the same process. It withholds differences on a changed controller,
decreasing counter, missing reset age, or reset age too short to establish
continuity across the measured window. The reset-age clock has one-second
resolution. This is not atomic kernel telemetry or proof that the memory is
healthy. An observed zero is not the same as missing EDAC data. Engine
neither loads a driver nor accesses the write-only reset control. The
broader `inventory` already lists visible EDAC controller totals.
[Linux EDAC ABI](https://docs.kernel.org/6.18/admin-guide/abi-testing-files.html#abi-file-testing-sysfs-devices-edac)


For a selected controller in saved passive qualification, add
`eld-engine qualify passive --edac-controller mc0` or preview that option
with the host-side passive planner. This option takes a single snapshot
and adds no sleep. Omit it for the default report. Evidence correlation
matches the named controller to the requested one but does not
authenticate the hardware.

For offline validation, save the first command's stdout and transfer it to
the host running the checker:

```sh
eld-engine ram edac --controller mc0 > edac.json
python3 scripts/check_edac_health_evidence.py --report edac.json --controller mc0
```

Pass `--expected-sha256` with a digest obtained from a trusted capture to
bind the exact file bytes. The checker preserves the reported availability
and exact decimal counters. It does not authenticate the device or assess
memory health, and it accepts neither sampled windows nor DIMM/rank detail.

Use `eld-engine ram edac --controller mc0 --location dimm0` to record
one explicitly selected DIMM or rank and the controller's unknown-location
counts. The controller totals already include errors counted in those
details. Do not add the details to the totals or require an exact sum
across separate live reads. Location detail is a single observation
and cannot be combined with the time-window option.

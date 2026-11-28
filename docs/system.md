# System inventory, CPU, memory and pressure

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

## Passive Linux pressure snapshot

`eld-engine pressure` reads the CPU, memory and I/O files under
`/proc/pressure` once, without starting a workload or registering a kernel
trigger. The JSON preserves the three percentage windows as decimal strings
and exact cumulative stall time in microseconds. Missing resources are
separate from observed zero. System CPU `full` is not applicable even if
the kernel exposes its backward-compatibility zero. This is system-level
pressure, not a per-process measurement or a benchmark result.
See [Linux Pressure Stall Information](https://docs.kernel.org/accounting/psi.html).

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

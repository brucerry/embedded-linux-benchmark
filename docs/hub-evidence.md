# Passive diagnostic input for Hub

The Engine runs on the Linux device. Hub owns orchestration, retention and
correlation. This contract does not imply Hub GUI support or a deployment grant.

1. Run the digest-verified binary with `capabilities`. Only select optional
   system health if `contracts.passive_system_health.schema` is 1.
2. Choose a fresh, private directory for the run. Invoke the approved fixed
   argument list, for example
   `eld-engine qualify passive --include-system-health --run-id HEX32 --evidence-json NEW_PATH`.
   Substitute a fresh nonzero 32-character lowercase hexadecimal token and a new
   path. Use an explicit profile, NIC or NVMe selection only when approved.
3. Require the exact invocation to exit successfully before retrieving the
   new file. Enforce the advertised 1 MiB file limit and the schema versions.
   Match the token, report type and observation to the command output. Store a
   hash of the original file, an authenticated device/session association, and
   the controller receipt time independently of the device's wall clock.
4. Keep metric states as reported. Unavailable, invalid, inaccessible and an
   observed zero convey different information. NUMA memory sizes and pressure
   totals are decimal strings and must not round through floating point.
   Repeated snapshots can support trends but are not a prediction or verdict.

The optional `qualification.system_health` object contains read-only NUMA
topology and kernel pressure snapshots. Qualification schema v1 without the flag
is unchanged. No load, active accelerator operation, driver modification,
reflash or remediation is performed. Treat unknown contracts as unsupported.

## Accelerator error counters

Do not use the AMDGPU RAS sysfs error-count files as passive telemetry.
Some kernel/ASIC combinations reset hardware error counters as a side effect
of the read. A read-only file descriptor does not prevent this. This Engine
contract does not open those files or advertise live RAS support. Lab imports
of previously captured observations are a separate, offline operation.

[Linux 6.6 AMDGPU read path](https://github.com/torvalds/linux/blob/v6.6/drivers/gpu/drm/amd/amdgpu/amdgpu_ras.c#L600-L622)

When `contracts.offline_amdgpu_ras.schema` is 1, Hub can decode previously
captured AMDGPU RAS text locally via the fixed argument list
`accelerator amdgpu-ras-decode --capture-text TEXT`. Provide the capture
text as a literal argument (128 ASCII bytes maximum), not a path or shell
fragment. Never read live sysfs RAS attributes to supply this command.
`decoded` is a parsing status; it says nothing about the device or when the
text was collected. Keep provenance outside this output, do not treat it as a
live metric, and never calculate a hardware-counter delta from this alone.

An Engine advertising `contracts.passive_pcie_health.schema=1` accepts the
optional `--pcie-health-device BDF` argument. A second opt-in,
`--pcie-health-sample-ms` (1–2000), takes two reads without applying load.
The evidence preserves actual monotonic interval and separates endpoint
observations, upstream observations, and root-hierarchy totals. Do not
sum those scopes, attribute a partner's errors to the endpoint, or compare
windows across a boot/device change. A missing requested BDF is a command
error; use the board profile checks if the expectation is that a device
must exist. This does not measure PCIe throughput.

Pass `--plan` to the verifier with the trusted plan previously generated
by `scripts/plan_hub_passive.py` for this run. The verifier checks run ID,
evidence size, and the exact planned set of optional observations; a skipped
feature is not counted as present. This reference mode covers only the
planner's supported passive invocations. The CLI receipt adds `plan_sha256` only
when it verifies a plan, binding the exact supplied bytes for audit comparison.
The digest is not a signature. This mode does not run the plan, verify the
remote output path, or authenticate the plan or device.

For local contract testing, `scripts/check_hub_evidence.py` correlates
already retrieved, bounded stdout and evidence files with a supplied run ID.
It does not establish trusted transfer, device identity, or command exit
status. Those are required separately in Hub. Keep the original evidence.
An AER category-set change can be a previously omitted kernel category
becoming nonzero; preserve both snapshots and the not-comparable reason.
Never substitute zero or assign the new error to the endpoint from that
interval alone.

When `contracts.passive_pcie_expectation.schema` is 1, a caller can specify
`--require-pcie BDF=VVVV:DDDD` instead of `--board-profile`. Read
`inventory.required_pcie` for each expectation's result. A `mismatch`
means the actual IDs differ from the supplied IDs; it is not proof of a
hardware fault. For a demonstration, use a deliberately wrong expectation
on a known present device without changing the device. The observation is
reported with a successful command exit; Hub must inspect the row status.

The offline verifier checks unique device addresses and consistency between
the state and reported expected/observed IDs. A matching pair does not prove
that the capture came from that device. For an explicit check, pass the same
canonical BDF list to `plan_hub_passive.py --require-pcie BDF[=vvvv:dddd][,...]`.
The preview refuses absent capability; with `--plan` the verifier also matches
each expected BDF and ID pair in order. Other board profiles remain outside
this reference plan.

For a small offline interoperability demo, `scripts/summarize_hub_evidence.py`
groups up to eight previously retrieved, correlated report pairs by
caller-selected host labels. It does not discover devices or authenticate
those labels. It counts named PCIe expectation states per host only. The
same BDF on two hosts is not the same device. A report with no expectation
rows is not a pass. Keep each original report and its digest; do not use a
demo summary to trigger remediation.

The `pressure_series` capability supports a short passive comparison window.
Use `--include-system-health --pressure-series-samples N --pressure-series-gap-ms MS`
to include it in run-bound evidence. Both values are required: 2–8 samples,
100–1000 ms between reads. Each sample records its start and end relative to
that process's series start. Observations are not synchronized across resources,
hosts or runs. No workload or permanent collector is started. Do not interpret
a missing PSI metric as zero or compare the monotonic offsets across devices.

The offline verifier also checks that optional series metadata, sample count,
sequence and monotonic intervals agree with the evidence envelope. Matching
stdout alone cannot make malformed timing valid; these checks still do not
authenticate the host or attest that a process exited successfully.

To report two passive observations of one named interface, use
`--network-interface NAME --network-health-sample-ms MS` (1–2000 ms).
The existing single observation remains the default. Reported deltas require
matching interface and boot identity and usable counters; missing identity or
counter resets do not mean zero change. This option does not generate traffic
or measure throughput.

For this read window, the offline verifier requires the same named interface
in both snapshots, a measured interval within the enclosing run, and delta
availability consistent with the reported continuity. It does not validate
every counter's arithmetic or establish that the claimed link identity is real.

To include two passive Linux thermal observations in run-bound evidence,
select `--thermal-sample-ms MS` (1–2000 ms). The inventory's `thermal_window`
reports the measured interval and matches differences by observed sensor
identity. Missing, changed, or unsupported sensors do not imply zero change.
No heating load is started, and the window is not synchronized with NIC or
pressure observations.

Where the Linux powercap driver exposes them, per-zone `power_microwatts`
and `max_power_range_microwatts` are decimal strings, distinct from the
energy counter and range. Some drivers expose only energy. An unavailable or
inaccessible power attribute is not zero. Do not sum parent and child zones,
interpret ranges as measurements, or treat one zone as whole-device power.

The reference verifier checks the requested interval, inventory baseline,
ordered zone set and signed difference arithmetic. It preserves
`not_comparable` because device identity is not exported in the report.
It does not authenticate the sensor or convert correlation into a hardware
qualification.

Use `--include-timeline` to correlate the passive phases within a single
qualification invocation. The optional `observation_timeline` includes only
invoked phases in their actual sequential order, with start and finish offsets
measured in milliseconds from one process-local monotonic clock. Equal offsets
are possible for sub-millisecond reads. A phase's presence does not establish
device availability or synchronize measurements across hosts.

The v1 verifier checks that the phase list matches the report sections actually
present. It rejects missing, duplicated, unexpected, overlapping or misordered
phases and offsets beyond the enclosing evidence duration. A missing timeline
remains valid for previous reports or runs that did not request the option.

## Offline command preview

`scripts/plan_hub_passive.py` turns a bounded, previously captured capability
report into a fixed argument array. It refuses missing required v1 contracts
and marks unadvertised optional `system-health` or `timeline` as unsupported;
it never executes a command. A preview does not authenticate the captured
report or establish that the nominated report directory is fresh and private.
The controller must verify those independently and confirm device/run/exit
identity before accepting evidence.

The selected EDAC option takes only a passive snapshot and is accepted
only when an Engine advertises `passive_edac_health`. New independent
contracts advertise saved PCIe snapshots, explicit peer receipts, and
single-line offline Jetson decoding. They describe software support only.
They do not imply a device exists, authorize workload execution, or make
raw continuity identifiers suitable for logging or upload.


## Fixture-backed NVIDIA capture format

`accelerator nvidia-health-decode --device BDF --capture-text TEXT` is an
offline decoder, not a GPU probe. Its adapter record is one bounded ASCII
line: `eld_nvml_v1,BDF,TEMP_C,POWER_MW,CORRECTED_VOLATILE_TOTAL,UNCORRECTED_VOLATILE_TOTAL`.
Each metric is an unsigned decimal or one of `unsupported`, `unavailable`
and `inaccessible`. Example synthetic record (not a GPU observation):
`eld_nvml_v1,00000000:65:00.0,45,40000,0,unsupported`.
Use the separately selected Linux PCI address `0000:65:00.0` for this example.
The decoder preserves counter precision and refuses mismatched addresses, but
cannot authenticate the device, collector or time. It does not accept a path,
run a vendor tool, attach to a workload, or read live NVML. The power value's
sampling behavior depends on the collecting hardware and vendor API.

Render `ecc_scope: volatile_total` as a single-capture counter, not an
interval or lifetime count. A Linux driver reload can reset it in the same
boot. This format carries no driver-load identity, so do not derive a delta
from it alone, even if the later total is higher. See
[optional NVIDIA health capture](gpu.md#optional-nvidia-health-capture).

[NVIDIA NVML device queries](https://docs.nvidia.com/deploy/nvml-api/latest/api/group__nvmlDeviceQueries.html)

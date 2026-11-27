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

For local contract testing, `scripts/check_hub_evidence.py` correlates
already retrieved, bounded stdout and evidence files with a supplied run ID.
It does not establish trusted transfer, device identity, or command exit
status. Those are required separately in Hub. Keep the original evidence.
An AER category-set change can be a previously omitted kernel category
becoming nonzero; preserve both snapshots and the not-comparable reason.
Never substitute zero or assign the new error to the endpoint from that
interval alone.

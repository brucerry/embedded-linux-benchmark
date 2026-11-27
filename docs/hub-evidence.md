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

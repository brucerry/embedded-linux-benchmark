# Engine hardware smoke checklist

Record the Engine commit, binary hash, board model, kernel and firmware before
interpreting a result. Run only checks that are available in that build and
within the approved test scope. Use `unavailable` for unsupported telemetry and
`not run` for missing fixtures or permissions; neither is a pass.

| Test | Evidence to retain | When to run |
| --- | --- | --- |
| ARM64 OpenWrt startup and passive inventory | Verified binary hash, exit status, capabilities and a fresh `qualify passive` evidence file matching stdout | Frozen smoke candidate; no workload. |
| Reported image build and kernel labels | A private inventory evidence file and a check against an independently supplied profile. Record match, mismatch, or incomplete; do not treat a matching label as proof of flashed bytes. | Optional separate development run after baseline smoke; see [profile check](#optional-image-label-check). |
| PCIe passive inventory and link | Observed BDF, IDs, driver, negotiated speed/width, and counter availability. Compare IDs only with an independently known board profile. If the target has no enumerated PCIe endpoint, record unavailable. | Frozen candidate; use `eld-engine pcie health --device BDF` only for a confirmed enumerated target. |
| PCIe window and counter comparison | Observer-specific increments or explicit non-comparable status. Record `counter_coverage`: exit 0 can mean comparable records with all counters unavailable. CI distinguishes that case from observed zero and covers changed boot/device/binding and counter decreases. Never induce hardware errors, reset, or rebind during passive smoke. | Separate development build; use `--sample-ms` for a passive window or two opt-in saved snapshots for offline comparison. |
| Linux CPU, memory and I/O pressure | Preserve unavailable separately from observed zero and CPU `full` as not applicable. The offline checker refuses a series that differs from its approved count or gap. | Separate development build. Two passive samples with a 100 ms gap are enough for scope and report validation, not a performance test. See [pressure series](system.md#bounded-passive-pressure-series). |
| NVIDIA GPU health | PCIe identity, reported metrics and unsupported fields. Run the synthetic collector-to-CLI check first; it does not establish physical results. | Separate NVIDIA lab host; no active GPU workload in passive smoke. |
| NIC peer and NVMe health | Matching sender and receiver receipts, separate route lookups, counters and measured behavior on an isolated link with a controlled peer; read-only NVMe health from an explicitly identified controller. Check saved SMART page and offline decode digests when a separate capture exists. Storage writes require a dedicated scratch path and separate scope. | Separate fixture run; not a prerequisite for baseline OpenWrt smoke. |

Keep raw output and the evidence validation result. Clean only the run's own
temporary copies. A successful smoke result is not a performance or reliability
qualification.

[PCIe](pcie.md) · [GPU](gpu.md) · [Networking](network.md) ·
[Storage](storage.md) · [Evidence handoff](hub-evidence.md)

For a separate development run, the board bring-up preview can include one
explicit NIC window and one explicit NVMe controller if the Engine advertises
both, for example `plan_board_bringup.py --network-interface eth0
--network-sample-ms 1000 --nvme-controller nvme0` with the usual capability,
run ID, and evidence path arguments. Nothing is run by the preview. Unsupported
is distinct from not requested; neither probe is part of the frozen baseline.

### Optional PCIe plan/evidence smoke

For a separate development run, pass a known, approved local PCIe BDF and an
absolute path to a trusted Engine build. Replace the sample BDF with the one
identified for your test fixture:

```sh
python3 -B tests/hub_passive_smoke.py /absolute/path/to/eld-engine \
  --pcie-health-device 0000:00:00.0
```

This adds a 5 ms, two-read passive PCIe check to the standard smoke suite.
It checks that the saved evidence matches the planned device and that an
otherwise identical plan with a different interval is rejected. Without the
flag the suite does not select a PCIe device. Passing this check does not prove
that AER counters are exposed, that no errors occurred, or that PCIe throughput
was measured. Do not substitute this development test for the frozen OpenWrt
baseline.

### Optional image label check

On the separately approved target, record the binary identity and capture
a fresh private report with the development build:

```sh
work=$(mktemp -d)
eld-engine inventory --evidence-json "$work/inventory.json"
```

After a successful exit, retain the report and the expected labels from the
image build or board team **separately**. Do not create the expected labels
from the target report. Check the saved report on the controller:

```sh
python3 -B scripts/check_inventory_profile.py \
  --inventory inventory.json --profile expected.json
```

For OpenWrt, the optional `distribution_build_id` is the reported build
revision. Keep it separate from `distribution_version`, `kernel_release`,
and any DMI firmware version. Use [the profile format](system.md#check-explicit-platform-expectations)
and inspect the JSON status rather than treating exit zero as a match.
An older capture without the label is incomplete if the profile requires it.
Never substitute this optional check or a development binary for the
frozen ARM64 board smoke packet.

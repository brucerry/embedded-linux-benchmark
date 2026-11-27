# Engine hardware smoke checklist

Record the Engine commit, binary hash, board model, kernel and firmware before
interpreting a result. Run only checks that are available in that build and
within the approved test scope. Use `unavailable` for unsupported telemetry and
`not run` for missing fixtures or permissions; neither is a pass.

| Test | Evidence to retain | When to run |
| --- | --- | --- |
| ARM64 OpenWrt startup and passive inventory | Verified binary hash, exit status, capabilities and a fresh `qualify passive` evidence file matching stdout | Frozen smoke candidate; no workload. |
| PCIe passive inventory and link | Observed BDF, IDs, driver, negotiated speed/width, and counter availability. Compare IDs only with an independently known board profile. If the target has no enumerated PCIe endpoint, record unavailable. | Frozen candidate; use `eld-engine pcie health --device BDF` only for a confirmed enumerated target. |
| PCIe window and counter comparison | Observer-specific increments or explicit non-comparable status. Record `counter_coverage`: exit 0 can mean comparable records with all counters unavailable. CI distinguishes that case from observed zero and covers changed boot/device/binding and counter decreases. Never induce hardware errors, reset, or rebind during passive smoke. | Separate development build; use `--sample-ms` for a passive window or two opt-in saved snapshots for offline comparison. |
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

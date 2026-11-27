#!/usr/bin/env python3
"""Compose a passive bring-up preview using captured Engine capabilities."""
import argparse
import json

from check_hub_evidence import (
    EDAC_CONTROLLER, MMC_CARD, NVME_CONTROLLER, PCI_BDF, check, parse, pcie_expectations, read_bounded, valid_nic_interface,
)
from plan_hub_passive import plan, supports_pcie_health, supports_thermal_wait, valid_thermal_wait


def supports(contracts, key, flag, scope=None):
    row = contracts.get(key)
    return (type(row) is dict and type(row.get("schema")) is int and row["schema"] == 1
            and row.get("flag") == flag and (scope is None or row.get("scope") == scope))


def preview(capabilities, run_id, evidence_path, expected_pcie=None, edac_controller=None,
            network_interface=None, network_sample_ms=None, nvme_controller=None, mmc_card=None, thermal_sample_ms=None, pcie_health_device=None, pcie_health_sample_ms=None):
    # Validate requests even when a feature cannot be planned. Do not treat bad
    # values as a harmless skip or infer a hardware target from capabilities.
    if expected_pcie is not None:
        pcie_expectations(expected_pcie)
    if edac_controller is not None:
        check(type(edac_controller) is str and EDAC_CONTROLLER.fullmatch(edac_controller),
              "invalid requested EDAC controller")
    nic_requested = network_interface is not None or network_sample_ms is not None
    if nic_requested:
        check(valid_nic_interface(network_interface) and type(network_sample_ms) is int
              and 1 <= network_sample_ms <= 2000, "provide one interface and a bounded NIC window")
    if nvme_controller is not None:
        check(type(nvme_controller) is str and NVME_CONTROLLER.fullmatch(nvme_controller),
              "invalid requested NVMe controller")
    if mmc_card is not None:
        check(type(mmc_card) is str and MMC_CARD.fullmatch(mmc_card), "invalid requested MMC card")
    if thermal_sample_ms is not None:
        check(valid_thermal_wait(thermal_sample_ms), "invalid requested thermal window")
    if pcie_health_sample_ms is not None:
        check(pcie_health_device is not None and type(pcie_health_sample_ms) is int
              and 1 <= pcie_health_sample_ms <= 2000, "provide a PCIe device and a bounded wait")
    if pcie_health_device is not None:
        check(type(pcie_health_device) is str and PCI_BDF.fullmatch(pcie_health_device),
              "invalid requested PCIe health device")
    check(type(capabilities) is dict, "unsupported capabilities")
    contracts = capabilities.get("contracts")
    check(type(contracts) is dict, "unsupported capabilities")
    pcie = expected_pcie if supports(contracts, "passive_pcie_expectation", "--require-pcie") else None
    edac = edac_controller if supports(contracts, "passive_edac_health", "--edac-controller",
                                       "selected_snapshot") else None
    nvme = nvme_controller if supports(contracts, "passive_nvme_controller", "--nvme-controller",
                                       "selected_sysfs_attributes_only") else None
    mmc = mmc_card if supports(contracts, "passive_mmc_health", "--mmc-card", "cached_sysfs_only") else None
    nic_row = contracts.get("passive_network_health")
    nic = (network_interface if nic_requested and type(nic_row) is dict
           and type(nic_row.get("schema")) is int and nic_row["schema"] == 1
           and nic_row.get("qualification_flag") == "--network-health-sample-ms"
           and type(nic_row.get("max_sample_wait_ms")) is int
           and network_sample_ms <= nic_row["max_sample_wait_ms"] <= 2000 else None)
    thermal = thermal_sample_ms if supports_thermal_wait(contracts, thermal_sample_ms) else None
    pcie_health = pcie_health_device if supports_pcie_health(contracts, pcie_health_sample_ms) else None
    pcie_wait = pcie_health_sample_ms if pcie_health is not None else None
    passive = plan(capabilities, run_id, evidence_path, ["system-health", "timeline"], pcie, edac,
                   nic, network_sample_ms if nic is not None else None, nvme, mmc, thermal, pcie_health, pcie_wait)
    steps = [{"id": "inventory", "status": "planned"}]
    steps.extend({"id": row["feature"], "status": row["status"]} for row in passive["optional"])
    for ident, requested, selected in (("expected_pcie", expected_pcie, pcie),
                                       ("selected_edac", edac_controller, edac),
                                       ("selected_nic", network_interface, nic),
                                       ("selected_nvme", nvme_controller, nvme),
                                       ("selected_mmc", mmc_card, mmc)):
        steps.append({"id": ident, "status": "not_requested" if requested is None else
                      "planned" if selected is not None else "unsupported"})
    thermal_step = {
        "id": "selected_thermal_window",
        "status": "not_requested" if thermal_sample_ms is None else
                  "planned" if thermal is not None else "unsupported",
    }
    if thermal is not None:
        thermal_step.update(requested_ms=thermal, reading_scope="kernel_reported",
                            sensor_provenance="not_verified")
    steps.append(thermal_step)
    pcie_step = {
        "id": "selected_pcie_health",
        "status": "not_requested" if pcie_health_device is None else
                  "planned" if pcie_health is not None else "unsupported",
    }
    if pcie_health is not None:
        pcie_step.update(device=pcie_health, scope="single_passive_snapshot" if pcie_wait is None
                         else "two_passive_snapshots", hardware_authenticity="not_verified")
        if pcie_wait is not None:
            pcie_step["requested_ms"] = pcie_wait
    steps.append(pcie_step)
    for ident in ("cpu_ram_load", "storage_write", "network_peer", "hardware_fixture"):
        steps.append({"id": ident, "status": "skipped", "reason": "passive_profile"})
    return {"schema_version": 1, "kind": "board_bringup_preview", "execution": "none",
            "hardware_availability": "not_checked", "steps": steps, "passive_plan": passive}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--capabilities", required=True)
    cli.add_argument("--run-id", required=True)
    cli.add_argument("--evidence-path", required=True)
    cli.add_argument("--require-pcie")
    cli.add_argument("--edac-controller")
    cli.add_argument("--network-interface")
    cli.add_argument("--network-sample-ms", type=int)
    cli.add_argument("--nvme-controller")
    cli.add_argument("--mmc-card")
    cli.add_argument("--thermal-sample-ms", type=int)
    cli.add_argument("--pcie-health-device")
    cli.add_argument("--pcie-health-sample-ms", type=int)
    args = cli.parse_args()
    try:
        out = preview(parse(read_bounded(args.capabilities)), args.run_id, args.evidence_path,
                      args.require_pcie, args.edac_controller, args.network_interface,
                      args.network_sample_ms, args.nvme_controller, args.mmc_card, args.thermal_sample_ms,
                      args.pcie_health_device, args.pcie_health_sample_ms)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Preview rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

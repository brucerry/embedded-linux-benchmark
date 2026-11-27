#!/usr/bin/env python3
"""Compose a passive bring-up preview using captured Engine capabilities."""
import argparse
import json

from check_hub_evidence import EDAC_CONTROLLER, check, parse, pcie_expectations, read_bounded
from plan_hub_passive import plan


def supports(contracts, key, flag, scope=None):
    row = contracts.get(key)
    return (type(row) is dict and type(row.get("schema")) is int and row["schema"] == 1
            and row.get("flag") == flag and (scope is None or row.get("scope") == scope))


def preview(capabilities, run_id, evidence_path, expected_pcie=None, edac_controller=None):
    # Validate requests even when a feature cannot be planned. Do not treat bad
    # values as a harmless skip or infer a hardware target from capabilities.
    if expected_pcie is not None:
        pcie_expectations(expected_pcie)
    if edac_controller is not None:
        check(type(edac_controller) is str and EDAC_CONTROLLER.fullmatch(edac_controller),
              "invalid requested EDAC controller")
    contracts = capabilities.get("contracts")
    check(type(contracts) is dict, "unsupported capabilities")
    pcie = expected_pcie if supports(contracts, "passive_pcie_expectation", "--require-pcie") else None
    edac = edac_controller if supports(contracts, "passive_edac_health", "--edac-controller",
                                       "selected_snapshot") else None
    passive = plan(capabilities, run_id, evidence_path, ["system-health", "timeline"], pcie, edac)
    steps = [{"id": "inventory", "status": "planned"}]
    steps.extend({"id": row["feature"], "status": row["status"]} for row in passive["optional"])
    for ident, requested, selected in (("expected_pcie", expected_pcie, pcie),
                                       ("selected_edac", edac_controller, edac)):
        steps.append({"id": ident, "status": "not_requested" if requested is None else
                      "planned" if selected is not None else "unsupported"})
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
    args = cli.parse_args()
    try:
        out = preview(parse(read_bounded(args.capabilities)), args.run_id, args.evidence_path,
                      args.require_pcie, args.edac_controller)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Preview rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

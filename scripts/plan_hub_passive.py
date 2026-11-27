"""Preview a fixed passive Engine argv from already-verified capabilities.

This example neither runs a command nor sends anything to a device.
"""
import argparse
import json

from check_hub_evidence import (
    MAX_BYTES, OPTIONAL_PASSIVE, RUN_ID, check, check_evidence_path, parse, pcie_expectations, read_bounded,
)


def plan(capabilities, run_id, output_path, options, required_pcie=None):
    check(type(run_id) is str and RUN_ID.fullmatch(run_id) and run_id != "0" * 32,
          "invalid run ID")
    check_evidence_path(output_path)
    check(type(capabilities.get("capabilities_schema_version")) is int
          and capabilities["capabilities_schema_version"] == 1
          and capabilities.get("os_target") == "linux"
          and type(capabilities.get("contracts")) is dict, "unsupported capabilities")
    contracts = capabilities["contracts"]
    required = contracts.get("passive_qualification")
    check(type(required) is dict
          and all(type(required.get(k)) is int and required[k] == 1
                  for k in ("observation_schema", "evidence_schema"))
          and required.get("run_id") == "hex32"
          and type(required.get("max_evidence_bytes")) is int
          and 0 < required["max_evidence_bytes"] <= MAX_BYTES,
          "unsupported mandatory passive evidence contract")
    check(len(options) <= len(OPTIONAL_PASSIVE) and len(set(options)) == len(options)
          and all(k in OPTIONAL_PASSIVE for k in options), "unknown or duplicate optional feature")
    argv = ["qualify", "passive", "--run-id", run_id, "--evidence-json", output_path]
    if required_pcie is not None:
        pcie_expectations(required_pcie)
        advertised = contracts.get("passive_pcie_expectation")
        check(type(advertised) is dict and type(advertised.get("schema")) is int
              and advertised["schema"] == 1 and advertised.get("flag") == "--require-pcie",
              "Engine does not advertise the requested PCIe expectation")
        argv.extend(("--require-pcie", required_pcie))
    results = []
    for feature in options:
        name, flag, _field = OPTIONAL_PASSIVE[feature]
        advertised = contracts.get(name)
        supported = (type(advertised) is dict and type(advertised.get("schema")) is int
                     and advertised["schema"] == 1 and advertised.get("flag") == flag)
        results.append({"feature": feature, "status": "planned" if supported else "unsupported"})
        if supported:
            argv.append(flag)
    return {"plan_schema_version": 1, "argv": argv, "execution": "none",
            "max_evidence_bytes": required["max_evidence_bytes"], "optional": results}


def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--capabilities", required=True)
    args.add_argument("--run-id", required=True)
    args.add_argument("--evidence-path", required=True)
    args.add_argument("--feature", action="append", choices=tuple(OPTIONAL_PASSIVE), default=[])
    args.add_argument("--require-pcie", help="explicit canonical BDF[=vvvv:dddd] list")
    a = args.parse_args()
    try:
        result = plan(parse(read_bounded(a.capabilities)), a.run_id, a.evidence_path, a.feature, a.require_pcie)
    except (OSError, ValueError) as exc:
        args.exit(1, f"invalid: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

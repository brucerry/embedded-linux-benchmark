"""Preview a fixed passive Engine argv from already-verified capabilities.

This example neither runs a command nor sends anything to a device.
"""
import argparse
import json

from check_hub_evidence import (
    EDAC_CONTROLLER, MAX_BYTES, MMC_CARD, NVME_CONTROLLER, OPTIONAL_PASSIVE, RUN_ID, check, check_evidence_path, parse, pcie_expectations, read_bounded, valid_nic_interface,
)


def plan(capabilities, run_id, output_path, options, required_pcie=None, edac_controller=None,
         network_interface=None, network_sample_ms=None, nvme_controller=None, mmc_card=None, thermal_sample_ms=None):
    check(type(run_id) is str and RUN_ID.fullmatch(run_id) and run_id != "0" * 32,
          "invalid run ID")
    check_evidence_path(output_path)
    check(type(capabilities) is dict
          and type(capabilities.get("capabilities_schema_version")) is int
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
    if edac_controller is not None:
        advertised = contracts.get("passive_edac_health")
        check(type(edac_controller) is str and EDAC_CONTROLLER.fullmatch(edac_controller),
              "invalid requested EDAC controller")
        check(type(advertised) is dict and type(advertised.get("schema")) is int
              and advertised["schema"] == 1 and advertised.get("flag") == "--edac-controller"
              and advertised.get("scope") == "selected_snapshot",
              "Engine does not advertise selected EDAC snapshots")
        argv.extend(("--edac-controller", edac_controller))
    if network_interface is not None or network_sample_ms is not None:
        check(valid_nic_interface(network_interface) and type(network_sample_ms) is int
              and 1 <= network_sample_ms <= 2000, "provide one interface and a bounded NIC window")
        advertised = contracts.get("passive_network_health")
        check(type(advertised) is dict and type(advertised.get("schema")) is int
              and advertised["schema"] == 1
              and advertised.get("qualification_flag") == "--network-health-sample-ms"
              and type(advertised.get("max_sample_wait_ms")) is int
              and network_sample_ms <= advertised["max_sample_wait_ms"] <= 2000,
              "Engine does not advertise the requested NIC window")
        argv.extend(("--network-interface", network_interface,
                     "--network-health-sample-ms", str(network_sample_ms)))
    if nvme_controller is not None:
        check(type(nvme_controller) is str and NVME_CONTROLLER.fullmatch(nvme_controller),
              "invalid requested NVMe controller")
        advertised = contracts.get("passive_nvme_controller")
        check(type(advertised) is dict and type(advertised.get("schema")) is int
              and advertised["schema"] == 1 and advertised.get("flag") == "--nvme-controller"
              and advertised.get("scope") == "selected_sysfs_attributes_only",
              "Engine does not advertise passive NVMe controller selection")
        argv.extend(("--nvme-controller", nvme_controller))
    if mmc_card is not None:
        check(type(mmc_card) is str and MMC_CARD.fullmatch(mmc_card), "invalid requested MMC card")
        advertised = contracts.get("passive_mmc_health")
        check(type(advertised) is dict and type(advertised.get("schema")) is int
              and advertised["schema"] == 1 and advertised.get("flag") == "--mmc-card"
              and advertised.get("scope") == "cached_sysfs_only",
              "Engine does not advertise selected cached MMC observations")
        argv.extend(("--mmc-card", mmc_card))
    if thermal_sample_ms is not None:
        check(type(thermal_sample_ms) is int and 1 <= thermal_sample_ms <= 2000,
              "invalid requested thermal window")
        row = contracts.get("passive_thermal_window")
        check(type(row) is dict and type(row.get("schema")) is int and row["schema"] == 1
              and row.get("qualification_flag") == "--thermal-sample-ms"
              and type(row.get("max_sample_wait_ms")) is int
              and thermal_sample_ms <= row["max_sample_wait_ms"] <= 2000,
              "Engine does not advertise the requested thermal window")
        argv.extend(("--thermal-sample-ms", str(thermal_sample_ms)))
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
    args.add_argument("--edac-controller", help="one explicit EDAC controller")
    args.add_argument("--network-interface", help="one explicitly selected NIC interface")
    args.add_argument("--network-sample-ms", type=int, help="passive NIC window in milliseconds")
    args.add_argument("--nvme-controller", help="one explicitly selected NVMe controller; no SMART or raw I/O")
    args.add_argument("--mmc-card", help="one explicit MMC card; cached sysfs only")
    args.add_argument("--thermal-sample-ms", type=int, help="optional thermal window, 1–2000 ms")
    a = args.parse_args()
    try:
        result = plan(parse(read_bounded(a.capabilities)), a.run_id, a.evidence_path, a.feature, a.require_pcie, a.edac_controller, a.network_interface, a.network_sample_ms, a.nvme_controller, a.mmc_card, a.thermal_sample_ms)
    except (OSError, ValueError, RecursionError) as exc:
        args.exit(1, f"invalid: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

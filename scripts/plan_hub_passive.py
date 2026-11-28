"""Preview a fixed passive Engine argv from already-verified capabilities.

This example neither runs a command nor sends anything to a device.
"""
import argparse
import json

from check_hub_evidence import (
    EDAC_CONTROLLER, MAX_BYTES, MMC_CARD, NVME_CONTROLLER, OPTIONAL_PASSIVE, PCI_BDF, RUN_ID, check, check_evidence_path, parse, pcie_expectations, read_bounded, valid_nic_interface,
)


def valid_thermal_wait(wait):
    return type(wait) is int and 1 <= wait <= 2000


def supports_thermal_wait(contracts, wait):
    row = contracts.get("passive_thermal_window")
    return (valid_thermal_wait(wait) and type(row) is dict and type(row.get("schema")) is int
            and row["schema"] == 1 and row.get("qualification_flag") == "--thermal-sample-ms"
            and type(row.get("max_sample_wait_ms")) is int
            and wait <= row["max_sample_wait_ms"] <= 2000)


def supports_pcie_health(contracts, wait=None):
    row = contracts.get("passive_pcie_health")
    return (type(row) is dict and type(row.get("schema")) is int and row["schema"] == 1
            and type(row.get("max_sample_wait_ms")) is int
            and 1 <= row["max_sample_wait_ms"] <= 2000
            and (wait is None or (type(wait) is int and 1 <= wait <= row["max_sample_wait_ms"])))


def plan(capabilities, run_id, output_path, options, required_pcie=None, edac_controller=None,
         network_interface=None, network_sample_ms=None, nvme_controller=None, mmc_card=None, thermal_sample_ms=None, pcie_health_device=None,
         pcie_health_sample_ms=None, pressure_samples=None, pressure_gap_ms=None):
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
        check(valid_thermal_wait(thermal_sample_ms), "invalid requested thermal window")
        check(supports_thermal_wait(contracts, thermal_sample_ms),
              "Engine does not advertise the requested thermal window")
        argv.extend(("--thermal-sample-ms", str(thermal_sample_ms)))
    if pcie_health_sample_ms is not None:
        check(pcie_health_device is not None and type(pcie_health_sample_ms) is int
              and 1 <= pcie_health_sample_ms <= 2000, "provide a PCIe device and a bounded wait")
    if pcie_health_device is not None:
        check(type(pcie_health_device) is str and PCI_BDF.fullmatch(pcie_health_device),
              "invalid requested PCIe health device")
        check(supports_pcie_health(contracts),
              "Engine does not advertise passive PCIe health schema 1")
        argv.extend(("--pcie-health-device", pcie_health_device))
        if pcie_health_sample_ms is not None:
            check(supports_pcie_health(contracts, pcie_health_sample_ms),
                  "Engine does not advertise the requested PCIe health wait")
            argv.extend(("--pcie-health-sample-ms", str(pcie_health_sample_ms)))
    wants_series = pressure_samples is not None or pressure_gap_ms is not None
    if wants_series:
        check(type(pressure_samples) is int and 2 <= pressure_samples <= 8
              and type(pressure_gap_ms) is int and 100 <= pressure_gap_ms <= 1000,
              "provide bounded pressure series count and gap")
        advertised = contracts.get("pressure_series")
        check(type(advertised) is dict and type(advertised.get("schema")) is int
              and advertised["schema"] == 1
              and advertised.get("qualification_flag") == "--pressure-series-samples"
              and type(advertised.get("max_samples")) is int
              and pressure_samples <= advertised["max_samples"] <= 8
              and type(advertised.get("max_gap_ms")) is int
              and pressure_gap_ms <= advertised["max_gap_ms"] <= 1000,
              "Engine does not advertise the requested pressure series")
        argv.extend(("--pressure-series-samples", str(pressure_samples),
                     "--pressure-series-gap-ms", str(pressure_gap_ms)))
    results = []
    for feature in options:
        name, flag, _field = OPTIONAL_PASSIVE[feature]
        advertised = contracts.get(name)
        supported = (type(advertised) is dict and type(advertised.get("schema")) is int
                     and advertised["schema"] == 1 and advertised.get("flag") == flag)
        results.append({"feature": feature, "status": "planned" if supported else "unsupported"})
        if supported:
            argv.append(flag)
    if wants_series:
        check(any(row == {"feature": "system-health", "status": "planned"} for row in results),
              "pressure series requires planned system health")
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
    args.add_argument("--pcie-health-device", help="one explicit PCIe BDF; single snapshot by default")
    args.add_argument("--pcie-health-sample-ms", type=int, help="optional PCIe window, 1–2000 ms")
    args.add_argument("--pressure-series-samples", type=int, help="passive pressure count, 2–8")
    args.add_argument("--pressure-series-gap-ms", type=int, help="passive pressure gap, 100–1000 ms")
    a = args.parse_args()
    try:
        result = plan(parse(read_bounded(a.capabilities)), a.run_id, a.evidence_path, a.feature, a.require_pcie, a.edac_controller, a.network_interface, a.network_sample_ms, a.nvme_controller, a.mmc_card, a.thermal_sample_ms, a.pcie_health_device, a.pcie_health_sample_ms, a.pressure_series_samples, a.pressure_series_gap_ms)
    except (OSError, ValueError, RecursionError) as exc:
        args.exit(1, f"invalid: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

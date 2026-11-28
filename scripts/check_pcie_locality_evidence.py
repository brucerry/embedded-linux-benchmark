#!/usr/bin/env python3
"""Check a saved PCIe locality observation for one selected function."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import PCI_BDF, check, parse, read_bounded

WITHHELD = ("unavailable", "inaccessible", "invalid")
MAX_REPORT = 128 * 1024


def verify(raw, device, expected_sha256=None):
    check(type(raw) is bytes and len(raw) <= MAX_REPORT, "PCIe locality report too large")
    check(type(device) is str and PCI_BDF.fullmatch(device), "expected a canonical PCIe BDF")
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256")
        check(sha == expected_sha256, "report differs from expected SHA-256")
    d = parse(raw)
    check(type(d) is dict and type(d.get("schema_version")) is int and d["schema_version"] == 1
          and d.get("kind") == "pcie_locality" and d.get("device") == device,
          "unsupported or differently selected PCIe locality report")
    base = {"schema_version", "kind", "device", "status"}
    status = d.get("status")
    check(type(status) is str and status in ("observed", "changed", *WITHHELD), "invalid device state")
    fields = {}
    if status == "observed":
        check(d.keys() == base | {"numa_node", "local_cpu_ids"}, "invalid locality fields")
        for key in ("numa_node", "local_cpu_ids"):
            row = d[key]
            check(type(row) is dict and type(row.get("status")) is str, "invalid locality observation")
            state = row["status"]
            if state == "observed":
                check(row.keys() == {"status", "value"}, "missing observed locality value")
                value = row["value"]
                if key == "numa_node":
                    check(type(value) is int and 0 <= value <= (1 << 31) - 1, "invalid NUMA node")
                else:
                    check(type(value) is list and len(value) <= 8192
                          and all(type(n) is int and 0 <= n <= (1 << 32) - 1 for n in value)
                          and all(a < b for a, b in zip(value, value[1:])),
                          "invalid or unordered local CPU hints")
            else:
                check(row.keys() == {"status"} and
                      state in (WITHHELD + ("unknown",) if key == "numa_node" else WITHHELD),
                      "invalid withheld locality value")
            fields[key] = row
    else:
        check(d.keys() == base, "unexpected locality data for a withheld device")
    return {"schema_version": 1, "kind": "offline_pcie_locality_check", "device": device,
            "status": status, "fields": fields, "report_sha256": sha,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "hardware_identity": "not_verified", "affinity": "not_assessed", "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--device", required=True)
    cli.add_argument("--expected-sha256")
    a = cli.parse_args()
    try:
        result = verify(read_bounded(a.report), a.device, a.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"PCIe locality evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Select one thermal zone from a saved two-snapshot inventory report."""
import argparse
import json
import re

from check_hub_evidence import check, parse, read_bounded, verify_thermal_window
from compare_inventory_evidence import capture

ZONE = re.compile(r"thermal_zone(0|[1-9][0-9]{0,6})")


def evaluate(raw, zone, expected=None):
    check(type(zone) is str and ZONE.fullmatch(zone), "expected an explicit canonical thermal zone")
    digest, _ = capture(raw, expected)
    evidence = parse(raw)
    inv = evidence["inventory"]
    check("thermal_window" in inv, "inventory does not contain a thermal window")
    verify_thermal_window(inv, evidence["elapsed_monotonic_ms"], max_wait_ms=60000)
    window = inv["thermal_window"]
    first, second = (next((r for r in window[phase]["zones"] if r["zone"] == zone), None)
                     for phase in ("before", "after"))
    delta = next((r["difference_millicelsius"] for r in window["differences"] if r["zone"] == zone),
                 {"status": "not_comparable"})
    status = ("not_reported" if first is None and second is None else
              "compared" if delta["status"] == "observed_difference" else "not_comparable")
    return {"schema_version": 1, "kind": "offline_selected_thermal_window",
            "zone": zone, "status": status, "inventory_sha256": digest,
            "requested_wait_ms": window["requested_wait_ms"],
            "sample_start_interval_ms": window["sample_start_interval_ms"],
            "before": first, "after": second, "difference_millicelsius": delta,
            "before_coverage": window["before"]["status"], "after_coverage": window["after"]["status"],
            "hardware_identity": "not_verified", "workload_execution": "none"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--inventory", required=True)
    cli.add_argument("--zone", required=True)
    cli.add_argument("--expected-inventory-sha256")
    a = cli.parse_args()
    try:
        out = evaluate(read_bounded(a.inventory), a.zone, a.expected_inventory_sha256)
    except (OSError, ValueError, KeyError, TypeError, RecursionError) as exc:
        cli.exit(1, f"Thermal evidence rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

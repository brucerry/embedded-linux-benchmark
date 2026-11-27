#!/usr/bin/env python3
"""Compare counters in two previously captured NVMe SMART pages without device I/O."""
import argparse
import json

from check_hub_evidence import check, parse, read_bounded
from check_nvme_smart_evidence import receipt

RAW_COUNTERS = ("power_on_hours_raw", "unsafe_shutdowns_raw", "media_errors_raw",
                "error_log_entries_raw")
UNIT_COUNTERS = ("data_units_read", "data_units_written")


def delta(before, after):
    if after < before:
        return {"status": "decreased_or_reset"}
    return {"status": "observed", "value": str(after - before)}


def compare(before, after):
    receipts = [receipt(*item) for item in (before, after)]
    check(receipts[0]["page_sha256"] != receipts[1]["page_sha256"],
          "identical pages do not establish two captures")
    a, b = (parse(item[1]) for item in (before, after))
    counters = {key: delta(int(a[key]), int(b[key])) for key in RAW_COUNTERS}
    for key in UNIT_COUNTERS:
        first, second = a[key], b[key]
        counters[key] = (
            delta(int(first["raw"]), int(second["raw"]))
            if first["status"] == second["status"] == "reported"
            else {"status": "not_comparable", "reason": "unreported_endpoint"}
        )
    return {
        "schema_version": 1, "kind": "offline_nvme_smart_counter_comparison",
        "device_sampled": False, "drive_health": "not_assessed",
        "capture_identity": "not_verified", "capture_order": "caller_supplied",
        "measurement_scope": "counter_arithmetic_only",
        "before": receipts[0], "after": receipts[1], "counters": counters,
    }


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for label in ("before", "after"):
        cli.add_argument("--" + label, nargs=3, required=True,
                         metavar=("SAVED_PAGE", "SAVED_REPORT", "SHA256"))
    args = cli.parse_args()
    try:
        result = compare(*[(read_bounded(page), read_bounded(report), sha)
                           for page, report, sha in (args.before, args.after)])
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

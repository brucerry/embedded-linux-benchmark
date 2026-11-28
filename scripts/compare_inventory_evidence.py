#!/usr/bin/env python3
"""Compare selected platform fields in two saved Engine inventory reports."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

FIELDS = ("distribution_id", "distribution_version", "kernel_release", "device_tree_model",
          "dmi_system_model", "dmi_board_model", "dmi_firmware_version")
OPTIONAL = ("distribution_build_id",)
WITHHELD = ("unavailable", "inaccessible", "invalid")
ENVELOPE = {"evidence_schema_version", "kind", "started_unix_ms", "finished_unix_ms",
            "elapsed_monotonic_ms", "inventory"}


def capture(raw, expected):
    digest = hashlib.sha256(raw).hexdigest()
    if expected is not None:
        check(type(expected) is str and re.fullmatch(r"[0-9a-f]{64}", expected),
              "expected lowercase inventory SHA-256")
        check(digest == expected, "inventory differs from selected SHA-256")
    data = parse(raw)
    check(type(data) is dict and data.keys() == ENVELOPE
          and type(data["evidence_schema_version"]) is int and data["evidence_schema_version"] == 1
          and data["kind"] == "passive_inventory", "unsupported inventory evidence")
    x = data["inventory"]
    check(type(x) is dict and type(x.get("schema_version")) is int and x["schema_version"] == 1,
          "unsupported inventory payload")
    for name in ("started_unix_ms", "finished_unix_ms", "elapsed_monotonic_ms"):
        n = data[name]
        check((name != "elapsed_monotonic_ms" and n is None)
              or (type(n) is int and 0 <= n <= (1 << 64) - 1), "invalid inventory timing")
    arch = x.get("process_architecture")
    check(type(arch) is str and re.fullmatch(r"[a-z0-9_]{1,32}", arch),
          "invalid reported architecture")
    fields = {"process_architecture": {"status": "observed", "value": arch}}
    for name in (*FIELDS, *OPTIONAL):
        if name in OPTIONAL and name not in x:
            fields[name] = {"status": "unavailable"}
            continue
        row = x.get(name)
        check(type(row) is dict and type(row.get("status")) is str,
              "invalid observation: " + name)
        status = row["status"]
        if status == "observed":
            val = row.get("value")
            check(row.keys() == {"status", "value"} and type(val) is str and val
                  and len(val.encode("utf-8")) <= 256
                  and not any(ord(c) < 32 or 127 <= ord(c) < 160 for c in val),
                  "invalid observed value: " + name)
        else:
            check(row.keys() == {"status"} and status in WITHHELD, "invalid status: " + name)
        fields[name] = row
    return digest, fields


def compare(before, after, expected_before=None, expected_after=None):
    check((expected_before is None) == (expected_after is None), "provide both expected digests")
    sha1, fields1 = capture(before, expected_before)
    sha2, fields2 = capture(after, expected_after)
    rows = {}
    for name, first in fields1.items():
        second = fields2[name]
        observed = first["status"] == second["status"] == "observed"
        rows[name] = {"status": ("unchanged" if first == second else "changed") if observed
                      else "not_comparable", "before": first, "after": second}
    return {"schema_version": 1, "kind": "offline_inventory_comparison",
            "before_sha256": sha1, "after_sha256": sha2, "capture_order": "caller_supplied",
            "hardware_identity": "not_verified", "boot_continuity": "not_assessed",
            "device_sampled": False, "fields": rows}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--before", required=True)
    cli.add_argument("--after", required=True)
    cli.add_argument("--expected-before-sha256")
    cli.add_argument("--expected-after-sha256")
    args = cli.parse_args()
    try:
        report = compare(read_bounded(args.before), read_bounded(args.after),
                         args.expected_before_sha256, args.expected_after_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Inventory comparison rejected: {exc}\n")
    print(json.dumps(report, sort_keys=True))


if __name__ == "__main__":
    main()

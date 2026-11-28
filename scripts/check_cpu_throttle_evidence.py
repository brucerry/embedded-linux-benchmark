#!/usr/bin/env python3
"""Validate one saved Linux thermal throttle counter snapshot; no aggregation."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

FIELDS = ("core_throttle_count", "package_throttle_count")
WITHHELD = ("unavailable", "inaccessible", "invalid")


def verify(raw, cpu, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= 4096, "invalid throttle evidence size")
    check(type(cpu) is int and 0 <= cpu <= 9_999_999, "invalid CPU selector")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "invalid selected digest")
        check(digest == expected_sha256, "report differs from selected SHA-256")
    record = parse(raw)
    check(type(record) is dict and type(record.get("schema_version")) is int
          and record["schema_version"] == 1 and record.get("kind") == "cpu_thermal_throttle"
          and type(record.get("cpu")) is int and record["cpu"] == cpu,
          "unexpected throttle schema or CPU")
    state = record.get("status")
    check(type(state) is str and state in ("observed", "changed", *WITHHELD),
          "invalid CPU state")
    fields = {}
    base = {"schema_version", "kind", "cpu", "status"}
    if state == "observed":
        check(record.keys() == base | set(FIELDS), "unexpected throttle fields")
        for key in FIELDS:
            row = record[key]
            check(type(row) is dict and type(row.get("status")) is str,
                  "invalid counter observation")
            if row["status"] == "observed":
                val = row.get("value")
                check(row.keys() == {"status", "value"} and type(val) is str
                      and re.fullmatch(r"0|[1-9][0-9]{0,19}", val)
                      and int(val) <= (1 << 64) - 1, "invalid event count")
            else:
                check(row.keys() == {"status"} and row["status"] in WITHHELD,
                      "invalid withheld counter")
            fields[key] = row
    else:
        check(record.keys() == base, "unobserved CPU cannot supply data")
    return {"schema_version": 1, "kind": "offline_cpu_throttle_check", "cpu": cpu,
            "status": state, "observations": fields, "report_sha256": digest,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "package_aggregation": "not_supported", "boot_continuity": "not_verified",
            "current_throttling": "not_assessed", "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--cpu", required=True, type=int)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        result = verify(read_bounded(args.report), args.cpu, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"CPU thermal throttle report rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

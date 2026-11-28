#!/usr/bin/env python3
"""Verify a saved Engine thermal cooling-device observation without sampling hardware."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

WITHHELD = ("unavailable", "inaccessible", "invalid")
FIELDS = ("type", "cur_state", "max_state")


def verify(raw, device, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= 4096, "expected bounded cooling evidence")
    check(type(device) is str and re.fullmatch(r"cooling_device(0|[1-9][0-9]{0,6})", device),
          "invalid selected cooling device")
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected lowercase SHA-256")
        check(sha == expected_sha256, "report differs from selected SHA-256")
    report = parse(raw)
    base = {"schema_version", "kind", "device", "status"}
    check(type(report) is dict and type(report.get("schema_version")) is int
          and report["schema_version"] == 1 and report.get("kind") == "thermal_cooling_state"
          and report.get("device") == device, "wrong cooling report schema or device")
    status = report.get("status")
    check(type(status) is str and status in ("observed", "changed", *WITHHELD),
          "invalid cooling device state")
    observations = {}
    if status == "observed":
        check(report.keys() == base | set(FIELDS), "unexpected cooling fields")
        for field in FIELDS:
            row = report[field]
            check(type(row) is dict and type(row.get("status")) is str, "invalid field status")
            if row["status"] == "observed":
                check(row.keys() == {"status", "value"} and type(row["value"]) is str,
                      "expected observed string")
                value = row["value"]
                if field == "type":
                    check(re.fullmatch(r"[ -~]{1,128}", value) and value == value.strip(),
                          "invalid cooling type")
                else:
                    check(re.fullmatch(r"0|[1-9][0-9]{0,19}", value)
                          and int(value) <= (1 << 64) - 1, "invalid cooling level")
            else:
                check(row.keys() == {"status"} and row["status"] in WITHHELD,
                      "invalid withheld field")
            observations[field] = row
    else:
        check(report.keys() == base, "non-observed cooling device cannot supply fields")
    return {"schema_version": 1, "kind": "offline_thermal_cooling_check", "device": device,
            "status": status, "observations": observations, "report_sha256": sha,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "cooling_health": "not_assessed", "throttling": "not_assessed",
            "hardware_identity": "not_verified", "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--device", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        result = verify(read_bounded(args.report), args.device, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Thermal cooling report rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Validate a saved AMDGPU RAS text decode, without reading any device."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

COUNTERS = {"corrected_errors", "uncorrected_errors"}


def verify(raw, expected_sha256=None):
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(digest == expected_sha256, "report differs from expected SHA-256")
    value = parse(raw)
    base = {"schema_version", "kind", "source", "capture_identity", "live_query", "status"}
    check(type(value) is dict and base <= value.keys()
          and type(value["schema_version"]) is int and value["schema_version"] == 1
          and value["kind"] == "amdgpu_ras_capture" and value["source"] == "provided_text"
          and value["capture_identity"] == "unverified" and value["live_query"] == "not_performed",
          "invalid offline AMDGPU RAS scope")
    status = value["status"]
    counts = {}
    if status == "decoded":
        check(value.keys() == base | COUNTERS, "invalid RAS counter fields")
        for key in sorted(COUNTERS):
            raw_value = value[key]
            check(type(raw_value) is str and re.fullmatch(r"0|[1-9][0-9]{0,19}", raw_value)
                  and int(raw_value) < 2**64, "invalid RAS counter value")
            counts[key] = raw_value
    else:
        check(value.keys() == base and status == "unavailable", "invalid withheld RAS report")
    return {"schema_version": 1, "kind": "offline_amdgpu_ras_check",
            "status": status, "counters": counts, "report_sha256": digest,
            "digest_selection": "matched" if expected_sha256 is not None else "unbound",
            "device_attribution": "not_present", "continuity": "not_assessed", "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--expected-sha256")
    arg = cli.parse_args()
    try:
        out = verify(read_bounded(arg.report), arg.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"AMDGPU RAS report rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

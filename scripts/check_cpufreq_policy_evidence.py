#!/usr/bin/env python3
"""Validate the saved observation of one CPU frequency policy."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

FIELDS = ("scaling_min_freq", "scaling_max_freq", "scaling_cur_freq", "cpuinfo_cur_freq")
WITHHELD = ("unavailable", "inaccessible", "invalid")


def valid_policy(policy):
    return type(policy) is str and re.fullmatch(r"policy(0|[1-9][0-9]{0,6})", policy) is not None


def verify(raw, policy, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= 4096, "invalid cpufreq report size")
    check(valid_policy(policy), "invalid policy selector")
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "invalid digest")
        check(sha == expected_sha256, "report differs from selected SHA-256")
    report = parse(raw)
    check(type(report.get("schema_version")) is int and report["schema_version"] == 1
          and report.get("kind") == "cpufreq_policy" and report.get("policy") == policy,
          "unsupported report or different policy")
    status = report.get("status")
    check(type(status) is str and status in ("observed", "changed", *WITHHELD), "invalid policy state")
    base = {"schema_version", "kind", "policy", "status"}
    fields = {}
    if status == "observed":
        check(report.keys() == base | {"unit", "fields"} and report["unit"] == "kHz",
              "unexpected policy fields or unit")
        fields = report["fields"]
        check(type(fields) is dict and fields.keys() == set(FIELDS), "unexpected frequency fields")
        for row in fields.values():
            check(type(row) is dict and type(row.get("status")) is str, "invalid reading")
            if row["status"] == "observed":
                value = row.get("value")
                check(row.keys() == {"status", "value"} and type(value) is str
                      and re.fullmatch(r"[1-9][0-9]{0,19}", value)
                      and int(value) <= (1 << 64) - 1, "unexpected kHz value")
            else:
                check(row.keys() == {"status"} and row["status"] in WITHHELD, "unexpected withheld value")
    else:
        check(report.keys() == base, "unobserved policy cannot contain readings")
    return {"schema_version": 1, "kind": "offline_cpufreq_policy_check", "policy": policy,
            "status": status, "fields": fields, "unit": "kHz", "report_sha256": sha,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "scaling_current_scope": "driver_reported", "throttling": "not_assessed",
            "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--policy", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        result = verify(read_bounded(args.report), args.policy, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"CPU frequency report rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Validate a saved observation of one explicitly selected HugeTLB pool."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

COUNTERS = ("nr_hugepages", "free_hugepages", "resv_hugepages", "surplus_hugepages")
WITHHELD = ("unavailable", "inaccessible", "invalid")


def verify(raw, size_kib, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= 8192, "expected bounded HugeTLB evidence")
    check(type(size_kib) is int and 1 <= size_kib <= 1 << 30
          and not (size_kib & (size_kib - 1)), "expected a valid selected page size")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256")
        check(digest == expected_sha256, "report differs from expected SHA-256")
    report = parse(raw)
    check(type(report) is dict and type(report.get("schema_version")) is int
          and report["schema_version"] == 1 and report.get("kind") == "hugetlb_pool"
          and type(report.get("size_kib")) is int and report["size_kib"] == size_kib,
          "unsupported or differently selected HugeTLB report")
    base = {"schema_version", "kind", "size_kib", "status"}
    state = report.get("status")
    check(type(state) is str and state in ("observed", "changed", *WITHHELD),
          "invalid pool status")
    counters = {}
    if state == "observed":
        check(report.keys() == base | {"counters"}, "unexpected observation fields")
        counters = report["counters"]
        check(type(counters) is dict and counters.keys() == set(COUNTERS),
              "expected all four counter observations")
        for counter in counters.values():
            check(type(counter) is dict and type(counter.get("status")) is str,
                  "invalid counter observation")
            if counter["status"] == "observed":
                value = counter.get("value")
                check(counter.keys() == {"status", "value"} and type(value) is str
                      and re.fullmatch(r"0|[1-9][0-9]{0,19}", value)
                      and int(value) <= (1 << 64) - 1, "invalid page count")
            else:
                check(counter.keys() == {"status"} and counter["status"] in WITHHELD,
                      "invalid withheld counter")
    else:
        check(report.keys() == base, "unexpected fields for an unobserved pool")
    return {"schema_version": 1, "kind": "offline_hugetlb_check", "size_kib": size_kib,
            "status": state, "counters": counters, "counter_unit": "pages",
            "report_sha256": digest, "digest_selection": "matched" if expected_sha256 else "unbound",
            "allocation": "not_assessed", "hardware_identity": "not_verified",
            "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--size-kib", required=True, type=int)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        result = verify(read_bounded(args.report), args.size_kib, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"HugeTLB evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Check a saved, single-controller EDAC snapshot offline; no hardware I/O."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import EDAC_CONTROLLER, check, parse, read_bounded, verify_edac_health


def verify(raw, controller, expected_sha256=None):
    check(type(controller) is str and EDAC_CONTROLLER.fullmatch(controller),
          "invalid expected EDAC controller")
    actual = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(actual == expected_sha256, "report differs from expected SHA-256")
    report = parse(raw)
    verify_edac_health(report)
    check(report["controller"] == controller, "EDAC controller differs from expected selection")
    return {
        "schema_version": 1, "kind": "offline_edac_snapshot_check",
        "controller": controller, "result": report["result"], "report_sha256": actual,
        "digest_selection": "matched" if expected_sha256 is not None else "unbound",
        "hardware_authenticity": "not_verified", "memory_health": "not_assessed",
    }


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--controller", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        out = verify(read_bounded(args.report), args.controller, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"EDAC evidence rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

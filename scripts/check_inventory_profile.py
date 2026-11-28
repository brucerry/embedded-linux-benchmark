#!/usr/bin/env python3
"""Compare a saved inventory with explicit exact platform expectations."""
import argparse
import hashlib
import json

from check_hub_evidence import check, parse, read_bounded
from compare_inventory_evidence import FIELDS, OPTIONAL, capture

ALLOWED = frozenset((*FIELDS, *OPTIONAL)) | {"process_architecture"}
MAX_PROFILE = 4096


def evaluate(raw, profile_bytes, expected_capture=None):
    check(len(profile_bytes) <= MAX_PROFILE, "inventory profile too large")
    profile = parse(profile_bytes)
    check(type(profile) is dict and profile.keys() == {"schema_version", "expected"}
          and type(profile["schema_version"]) is int and profile["schema_version"] == 1,
          "unsupported inventory profile")
    expected = profile["expected"]
    check(type(expected) is dict and expected and expected.keys() <= ALLOWED,
          "profile requires explicit supported field names")
    for key, value in expected.items():
        check(type(value) is str and 0 < len(value.encode("utf-8")) <= 256
              and not any(ord(c) < 32 or 127 <= ord(c) < 160 for c in value),
              "invalid expected platform label: " + key)
    digest, actual = capture(raw, expected_capture)
    rows = {}
    for key, label in sorted(expected.items()):
        observed = actual[key]
        if observed["status"] != "observed":
            state = "not_comparable"
        elif observed["value"] == label:
            state = "match"
        else:
            state = "mismatch"
        rows[key] = {"status": state, "expected": label, "observed": observed}
    states = {row["status"] for row in rows.values()}
    status = "mismatch" if "mismatch" in states else "incomplete" if "not_comparable" in states else "match"
    return {"schema_version": 1, "kind": "offline_inventory_profile_check", "status": status,
            "profile_sha256": hashlib.sha256(profile_bytes).hexdigest(), "inventory_sha256": digest,
            "hardware_identity": "not_verified", "qualification": "not_assessed", "fields": rows}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--inventory", required=True)
    cli.add_argument("--profile", required=True)
    cli.add_argument("--expected-inventory-sha256")
    a = cli.parse_args()
    try:
        result = evaluate(read_bounded(a.inventory), read_bounded(a.profile), a.expected_inventory_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Inventory profile check rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

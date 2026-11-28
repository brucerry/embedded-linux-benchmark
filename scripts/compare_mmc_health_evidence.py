#!/usr/bin/env python3
"""Compare exact cached MMC report codes without inferring wear or identity."""
import argparse
import json

from check_hub_evidence import check, parse, read_bounded
from check_mmc_health_evidence import RAW_FIELDS, verify


def compare(before, after, card, expected_before=None, expected_after=None):
    check((expected_before is None) == (expected_after is None), "provide both expected digests")
    first, second = (verify(raw, card, expected) for raw, expected in
                     ((before, expected_before), (after, expected_after)))
    records = (parse(before), parse(after))
    fields = {}
    for field in RAW_FIELDS:
        left, right = (r.get(field) for r in records)
        observed = (left is not None and right is not None
                    and left["status"] == right["status"] == "observed")
        row = {"status": ("unchanged" if left == right else "changed") if observed else "not_comparable",
               "before": left, "after": right}
        if field == "life_time_raw" and observed:
            row["code_changes"] = {key: left["value"][key] != right["value"][key] for key in left["value"]}
        fields[field] = row
    return {"schema_version": 1, "kind": "offline_mmc_report_comparison", "card": card,
            "before_sha256": first["report_sha256"], "after_sha256": second["report_sha256"],
            "before_card_observation": first["card_observation"],
            "after_card_observation": second["card_observation"],
            "capture_order": "caller_supplied", "hardware_identity": "not_verified",
            "wear_assessment": "not_assessed", "device_sampled": False, "fields": fields}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--before", required=True)
    cli.add_argument("--after", required=True)
    cli.add_argument("--card", required=True)
    cli.add_argument("--expected-before-sha256")
    cli.add_argument("--expected-after-sha256")
    a = cli.parse_args()
    try:
        result = compare(read_bounded(a.before), read_bounded(a.after), a.card,
                         a.expected_before_sha256, a.expected_after_sha256)
    except (ValueError, OSError, RecursionError) as exc:
        cli.exit(1, f"MMC comparison rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

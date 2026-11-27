#!/usr/bin/env python3
"""Check a saved passive MMC report against the intended card; no device I/O."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

CARD = re.compile(r"mmc(?:0|[1-9][0-9]{0,6}):[a-f0-9]{4}")
HASH = re.compile(r"[a-f0-9]{64}")
WITHHELD_FIELD = ("unavailable", "inaccessible", "invalid")
RAW_FIELDS = ("pre_eol_info_raw", "life_time_raw")


def byte(value):
    return type(value) is int and 0 <= value <= 255


def field(raw, valid):
    check(type(raw) is dict, "invalid MMC observation field")
    status = raw.get("status")
    if status == "observed":
        check(raw.keys() == {"status", "value"} and valid(raw["value"]),
              "invalid raw MMC observation")
    else:
        check(raw.keys() == {"status"} and status in WITHHELD_FIELD,
              "invalid withheld MMC observation")
    return status


def life(value):
    return (type(value) is dict and value.keys() == {"type_a_raw", "type_b_raw"}
            and all(byte(code) for code in value.values()))


def validate_snapshot(result, expected_card):
    check(type(expected_card) is str and CARD.fullmatch(expected_card), "invalid expected MMC card")
    check(type(result) is dict, "invalid MMC report object")
    base = {"schema_version", "card", "status"}
    check(base <= result.keys() and type(result["schema_version"]) is int
          and result["schema_version"] == 1 and result["card"] == expected_card,
          "unsupported MMC report or selected card mismatch")
    status = result["status"]
    fields = {}
    if status == "observed":
        check(result.keys() == base | {"card_type", *RAW_FIELDS} and result["card_type"] == "MMC",
              "invalid observed MMC report")
        fields = {name: field(result[name], validator)
                  for name, validator in zip(RAW_FIELDS, (byte, life))}
    else:
        check(result.keys() == base and status in (*WITHHELD_FIELD, "changed", "unsupported_card_type"),
              "invalid withheld MMC report")
    return status, fields


def verify(raw, expected_card, expected_sha256=None):
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and HASH.fullmatch(expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(sha == expected_sha256, "report differs from expected SHA-256")
    status, fields = validate_snapshot(parse(raw), expected_card)
    return {
        "schema_version": 1, "kind": "offline_mmc_report_check", "card": expected_card,
        "card_observation": status, "raw_field_states": fields, "report_sha256": sha,
        "digest_selection": "matched" if expected_sha256 is not None else "unbound",
        "hardware_authenticity": "not_verified", "wear_assessment": "not_assessed",
        "device_sampled": False,
    }


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--card", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        result = verify(read_bounded(args.report), args.card, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"MMC report rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

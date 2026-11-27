#!/usr/bin/env python3
"""Verify an offline NVMe SMART decode against one saved page and its expected digest."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, equal, parse, read_bounded


def receipt(page, report, expected_sha):
    check(len(page) == 512, "expected one 512-byte SMART log page")
    check(type(expected_sha) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha),
          "expected a lowercase SHA-256 digest")
    actual = hashlib.sha256(page).hexdigest()
    check(actual == expected_sha, "saved page differs from expected SHA-256")
    read = lambda offset, size: int.from_bytes(page[offset:offset + size], "little")

    def units(offset):
        value = read(offset, 16)
        return {"status": "reported" if value else "unreported", "raw": str(value)}

    expected = {
        "schema_version": 1,
        "source": "provided_smart_log_page",
        "device_sampled": False,
        "critical_warning_raw": page[0],
        "composite_temperature_kelvin": read(1, 2) or None,
        "available_spare_percent": page[3] if page[3] <= 100 else None,
        "available_spare_threshold_percent": page[4] if page[4] <= 100 else None,
        "percent_used_raw": page[5],
        "data_units_read": units(32),
        "data_units_written": units(48),
        **{key: str(read(offset, 16)) for key, offset in (
            ("power_on_hours_raw", 128), ("unsafe_shutdowns_raw", 144),
            ("media_errors_raw", 160), ("error_log_entries_raw", 176))},
    }
    check(equal(expected, parse(report)), "decoded report does not match the saved page")
    return {"schema_version": 1, "kind": "nvme_smart_decode_receipt", "status": "correlated",
            "page_bytes": len(page), "page_sha256": actual,
            "report_sha256": hashlib.sha256(report).hexdigest(), "device_sampled": False,
            "capture_provenance": "not_verified", "drive_health": "not_assessed"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--page", required=True, help="saved regular file, never a raw device")
    cli.add_argument("--report", required=True, help="saved output from Engine SMART decoder")
    cli.add_argument("--expected-sha256", required=True, help="trusted capture digest")
    args = cli.parse_args()
    try:
        result = receipt(read_bounded(args.page), read_bounded(args.report), args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

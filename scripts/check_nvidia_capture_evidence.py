#!/usr/bin/env python3
"""Validate a saved offline NVML decode for an explicitly selected PCI device."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import PCI_BDF, check, parse, read_bounded

FIELDS = {"temperature_celsius": 32, "power_milliwatts": 32,
          "corrected_ecc": 64, "uncorrected_ecc": 64}


def verify(raw, device, expected_sha256=None):
    check(type(device) is str and PCI_BDF.fullmatch(device), "expected a canonical PCIe BDF")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(digest == expected_sha256, "report differs from expected SHA-256")
    q = parse(raw)
    check(type(q) is dict and q.keys() == {
        "schema_version", "kind", "source", "capture_identity", "live_query", "bdf", "ecc_scope", *FIELDS
    } and type(q["schema_version"]) is int and q["schema_version"] == 1
        and q["kind"] == "nvidia_nvml_capture" and q["source"] == "provided_normalized_text"
        and q["capture_identity"] == "unverified" and q["live_query"] == "not_performed"
        and q["ecc_scope"] == "volatile_total", "invalid offline NVML report scope")
    check(q["bdf"] == device, "report PCI device differs from expected selection")
    for field, bits in FIELDS.items():
        row = q[field]
        check(type(row) is dict, "invalid NVML observation")
        if row.get("status") == "observed":
            value = row.get("value")
            check(row.keys() == {"status", "value"} and type(value) is str
                  and re.fullmatch(r"0|[1-9][0-9]{0,19}", value) and int(value) < 2**bits,
                  "invalid NVML observed value")
        else:
            check(row.keys() == {"status"} and row["status"] in ("unsupported", "unavailable", "inaccessible"),
                  "invalid NVML withheld value")
    return {"schema_version": 1, "kind": "offline_nvidia_capture_check", "bdf": device,
            "metrics": {k: q[k] for k in FIELDS}, "ecc_scope": q["ecc_scope"],
            "report_sha256": digest, "digest_selection": "matched" if expected_sha256 else "unbound",
            "hardware_authenticity": "not_verified", "ecc_continuity": "not_assessed",
            "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--device", required=True)
    cli.add_argument("--expected-sha256")
    arg = cli.parse_args()
    try:
        out = verify(read_bounded(arg.report), arg.device, arg.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"NVML report rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

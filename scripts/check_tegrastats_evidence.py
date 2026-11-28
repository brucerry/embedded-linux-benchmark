#!/usr/bin/env python3
"""Check one saved Engine Jetson GPU decode without accessing the device."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

GPU_FIELDS = ("gpu_utilization_percent", "gpu_frequency_mhz", "gpu_temperature_millicelsius")
U32 = (1 << 32) - 1
RAIL = re.compile(r"(?:VDD|VIN)_[A-Z0-9_]{1,44}")


def integer(value, low, high):
    return type(value) is int and low <= value <= high


def inspect(row, validate):
    check(type(row) is dict, "invalid observation")
    status = row.get("status")
    if status == "observed":
        check(row.keys() == {"status", "value"} and validate(row["value"]), "invalid observed value")
        return True
    check(status == "unavailable" and row.keys() == {"status"}, "invalid withheld observation")
    return False


def verify(raw, rail=None, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= 8192, "invalid Jetson report size")
    check(rail is None or (type(rail) is str and RAIL.fullmatch(rail)), "invalid selected rail")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "invalid selected digest")
        check(digest == expected_sha256, "report differs from the selected SHA-256")
    q = parse(raw)
    check(type(q) is dict and q.keys() == {
        "schema_version", "kind", "live_query", "capture_identity", "selected_rail", *GPU_FIELDS,
    } and type(q["schema_version"]) is int and q["schema_version"] == 1
        and q["kind"] == "jetson_tegrastats_capture" and q["live_query"] == "not_performed"
        and q["capture_identity"] == "unverified", "invalid offline Jetson report schema")
    metrics = {key: q[key] for key in GPU_FIELDS}
    checks = [
        inspect(metrics[GPU_FIELDS[0]], lambda v: integer(v, 0, 100)),
        inspect(metrics[GPU_FIELDS[1]], lambda v: type(v) is list and 1 <= len(v) <= 8
                and all(integer(n, 0, U32) for n in v)),
        inspect(metrics[GPU_FIELDS[2]], lambda v: integer(v, -999999, 999999)),
    ]
    power = q["selected_rail"]
    if rail is None:
        check(power is None, "report names a rail, but none was selected")
    else:
        check(type(power) is dict and power.keys() == {"observation", "label"}
              and power["label"] == rail, "report names a different rail")
        checks.append(inspect(power["observation"], lambda v: type(v) is dict
                              and v.keys() == {"instant_mw", "average_mw"}
                              and all(integer(n, 0, U32) for n in v.values())))
    check(any(checks), "report contains no observed telemetry")
    return {"schema_version": 1, "kind": "offline_tegrastats_check", "metrics": metrics,
            "selected_rail": power, "report_sha256": digest,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "capture_identity": "not_verified", "gpu_power_scope": "not_inferred",
            "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--rail")
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        out = verify(read_bounded(args.report), args.rail, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Jetson tegrastats report rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

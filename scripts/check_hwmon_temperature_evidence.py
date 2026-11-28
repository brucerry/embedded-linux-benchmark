#!/usr/bin/env python3
"""Validate one saved Engine temperature report for an explicitly selected channel."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

SENSOR = re.compile(r"hwmon(0|[1-9][0-9]{0,6})")
STATES = ("observed", "unavailable", "inaccessible", "invalid", "changed")


def verify(raw, sensor, channel, expected_sha256=None):
    check(type(raw) is bytes and len(raw) <= 4096, "invalid hwmon report bytes")
    check(type(sensor) is str and SENSOR.fullmatch(sensor), "invalid expected sensor")
    check(type(channel) is int and 1 <= channel <= 32, "invalid expected channel")
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected lowercase SHA-256")
        check(sha == expected_sha256, "report differs from expected SHA-256")
    d = parse(raw)
    check(type(d) is dict and type(d.get("schema_version")) is int and d["schema_version"] in (1, 2)
          and d.get("kind") == "hwmon_temperature", "unsupported hwmon report")
    check(d.get("sensor") == sensor and type(d.get("channel")) is int and d["channel"] == channel,
          "hwmon report does not match the selected sensor and channel")
    status = d.get("status")
    check(type(status) is str and status in STATES, "invalid sensor state")
    keys = {"schema_version", "kind", "sensor", "channel", "status"}
    version = d["schema_version"]
    if status == "observed":
        if version == 1:
            # Older development captures used a Celsius label that did not prove the ABI unit.
            keys.add("temperature_millicelsius")
            value = d.get("temperature_millicelsius")
        else:
            keys.update({"raw_value", "unit"})
            check(d.get("unit") == "unknown", "unexpected claimed hwmon unit")
            value = d.get("raw_value")
        check(type(value) is str and re.fullmatch(r"0|-?[1-9][0-9]{0,18}", value)
              and -(1 << 63) <= int(value) < 1 << 63, "invalid signed raw reading")
        if version == 1:
            check(-273150 <= int(value) <= 300000, "legacy reading exceeds original bounds")
    else:
        value = None
    check(d.keys() == keys, "inconsistent hwmon fields")
    return {"schema_version": 2, "kind": "offline_hwmon_temperature_check",
            "input_schema_version": version, "sensor": sensor, "channel": channel,
            "status": status, "raw_value": value, "unit": "unknown",
            "report_sha256": sha, "digest_selection": "matched" if expected_sha256 else "unbound",
            "hardware_identity": "not_verified", "device_sampled": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--sensor", required=True)
    cli.add_argument("--channel", type=int, required=True)
    cli.add_argument("--expected-sha256")
    a = cli.parse_args()
    try:
        out = verify(read_bounded(a.report), a.sensor, a.channel, a.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Hwmon report rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

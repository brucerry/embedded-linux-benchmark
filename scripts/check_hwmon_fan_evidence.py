#!/usr/bin/env python3
"""Check one saved Engine fan-speed report for an explicitly selected channel."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded
from check_hwmon_temperature_evidence import SENSOR, STATES


def verify(raw, sensor, channel, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= 4096, "invalid fan report bytes")
    check(type(sensor) is str and SENSOR.fullmatch(sensor), "invalid expected sensor")
    check(type(channel) is int and 1 <= channel <= 32, "invalid expected channel")
    sha = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected lowercase SHA-256")
        check(sha == expected_sha256, "report differs from expected SHA-256")
    d = parse(raw)
    check(type(d) is dict and type(d.get("schema_version")) is int and d["schema_version"] == 1
          and d.get("kind") == "hwmon_fan", "unsupported fan report")
    check(d.get("sensor") == sensor and type(d.get("channel")) is int and d["channel"] == channel,
          "fan report does not match the selected sensor and channel")
    status = d.get("status")
    check(type(status) is str and status in STATES, "invalid fan state")
    keys = {"schema_version", "kind", "sensor", "channel", "status"}
    value = None
    if status == "observed":
        keys.update({"raw_value", "unit"})
        value = d.get("raw_value")
        check(d.get("unit") == "rpm" and type(value) is str
              and re.fullmatch(r"0|[1-9][0-9]{0,19}", value)
              and int(value) <= (1 << 64) - 1, "invalid RPM reading")
    check(d.keys() == keys, "inconsistent fan fields")
    return {"schema_version": 1, "kind": "offline_hwmon_fan_check",
            "sensor": sensor, "channel": channel, "status": status,
            "rpm": value, "report_sha256": sha,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "cooling_health": "not_assessed", "hardware_identity": "not_verified",
            "device_sampled": False}


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
        cli.exit(1, f"Hwmon fan report rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

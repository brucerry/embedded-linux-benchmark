#!/usr/bin/env python3
"""Decode one saved Redfish Sensor temperature; no BMC access or actions."""
import argparse
from decimal import Decimal, DecimalException
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded
from check_redfish_system_capture import ID, CAP

TYPE = re.compile(r"#Sensor\.v1_[0-9]{1,3}_[0-9]{1,3}\.Sensor")


def read_sensor(raw, sensor_id, expected_sha256=None):
    check(type(raw) is bytes and 0 < len(raw) <= CAP, "invalid Redfish capture size")
    check(type(sensor_id) is str and ID.fullmatch(sensor_id), "invalid selected sensor ID")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "invalid digest selection")
        check(digest == expected_sha256, "capture differs from expected SHA-256")
    try:
        q = parse(raw)
    except DecimalException as exc:
        raise ValueError("invalid Redfish decimal") from exc
    check(type(q.get("@odata.type")) is str and TYPE.fullmatch(q["@odata.type"]),
          "expected a versioned Redfish Sensor")
    check(q.get("Id") == sensor_id, "capture differs from selected sensor ID")
    return q, digest


def isolated_reading(q, expected_type, accepted_units, value_field, minimum=Decimal(-1000000)):
    reading = q.get("Reading")
    if any(q.get(k) is None for k in ("ReadingType", "ReadingUnits", "Reading")):
        result = {"status": "unavailable"}
    elif q["ReadingType"] != expected_type or q["ReadingUnits"] not in accepted_units:
        result = {"status": "unsupported_type_or_unit"}
    elif type(reading) not in (int, Decimal):
        result = {"status": "invalid"}
    else:
        value = Decimal(reading)
        units = value.as_tuple()
        if (not value.is_finite() or len(units.digits) > 24 or abs(units.exponent) > 12
                or not minimum <= value <= 1000000):
            result = {"status": "invalid"}
        else:
            result = {"status": "reported", value_field: format(value, "f")}
    return result


def decode(raw, sensor_id, expected_sha256=None):
    q, digest = read_sensor(raw, sensor_id, expected_sha256)
    result = isolated_reading(q, "Temperature", ("Cel",), "value_celsius")
    return {"schema_version": 1, "kind": "redfish_saved_temperature_decode", "sensor_id": sensor_id,
            "reading": result, "capture_sha256": digest,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "physical_identity": "not_verified", "freshness": "not_assessed",
            "thermal_health": "not_assessed", "live_request": "not_performed",
            "actions_performed": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--capture", required=True)
    cli.add_argument("--sensor-id", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        out = decode(read_bounded(args.capture), args.sensor_id, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Redfish temperature capture refused: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Read a saved Redfish rotational Sensor in RPM, not fan controls or percent."""
import argparse
import json

from check_hub_evidence import read_bounded
from check_redfish_temperature_capture import isolated_reading, read_sensor


def decode(raw, sensor_id, expected_sha256=None):
    sensor, digest = read_sensor(raw, sensor_id, expected_sha256)
    reading = isolated_reading(sensor, "Rotational", ("{rev}/min", "RPM"), "value_rpm", minimum=0)
    return {"schema_version": 1, "kind": "redfish_saved_rotational_decode",
            "sensor_id": sensor_id, "reading": reading, "capture_sha256": digest,
            "digest_selection": "matched" if expected_sha256 else "unbound",
            "physical_identity": "not_verified", "freshness": "not_assessed",
            "cooling_health": "not_assessed", "live_request": "not_performed",
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
        cli.exit(1, f"Redfish rotational capture refused: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

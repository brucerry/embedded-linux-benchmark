#!/usr/bin/env python3
"""Decode selected fields from a saved Redfish ComputerSystem document."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

CAP = 65536
ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}")
TYPE = re.compile(r"#ComputerSystem\.v1_[0-9]{1,3}_[0-9]{1,3}\.ComputerSystem")
POWER = {"On", "Off", "PoweringOn", "PoweringOff", "Paused", "Sleeping", "Hibernating"}
HEALTH = {"OK", "Warning", "Critical"}


def decode(raw, system_id, expected_sha256=None):
    check(type(system_id) is str and ID.fullmatch(system_id), "invalid selected system ID")
    check(len(raw) <= CAP, "Redfish capture exceeds byte limit")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(expected_sha256 == digest, "capture differs from expected SHA-256")
    x = parse(raw)
    check(type(x) is dict and type(x.get("@odata.type")) is str and TYPE.fullmatch(x["@odata.type"]),
          "expected a versioned ComputerSystem document")
    check(x.get("Id") == system_id, "capture differs from selected system ID")
    status = x.get("Status")
    check(status is None or type(status) is dict, "invalid Redfish Status object")

    def observation(value, supported):
        if value is None:
            return {"status": "unavailable"}
        if type(value) is str and value in supported:
            return {"status": "reported", "value": value}
        return {"status": "unsupported_value"}

    status = status or {}
    return {"schema_version": 1, "kind": "redfish_saved_system_decode",
            "source": "provided_capture", "selected_system_id": system_id,
            "capture_sha256": digest,
            "digest_selection": "matched" if expected_sha256 is not None else "unbound",
            "physical_identity": "not_verified", "freshness": "not_assessed",
            "live_request": "not_performed", "actions_performed": False,
            "power_state": observation(x.get("PowerState"), POWER),
            "resource_health": observation(status.get("Health"), HEALTH),
            "health_rollup": observation(status.get("HealthRollup"), HEALTH)}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--capture", required=True)
    cli.add_argument("--system-id", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        out = decode(read_bounded(args.capture), args.system_id, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as e:
        cli.exit(1, f"Redfish capture refused: {e}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Recompute one saved passive NIC counter window; do not access any interface."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, equal, parse, read_bounded

KEYS = ("rx_bytes", "tx_bytes", "rx_errors", "tx_errors", "rx_dropped", "tx_dropped",
        "carrier_changes", "rx_crc_errors", "rx_missed_errors")
UNREADABLE = ("unavailable", "inaccessible", "invalid")
U64_MAX = (1 << 64) - 1


def decimal(raw):
    return (type(raw) is str and re.fullmatch(r"0|[1-9][0-9]{0,19}", raw) is not None
            and int(raw) <= U64_MAX)


def shape(value, fields, reason):
    check(type(value) is dict and value.keys() == set(fields), reason)


def field(value, valid):
    check(type(value) is dict, "invalid sample field")
    if value.get("status") == "observed":
        shape(value, ("status", "value"), "invalid observed field")
        check(valid(value["value"]), "invalid observed value")
        return value["value"]
    shape(value, ("status",), "invalid unavailable field")
    check(value["status"] in UNREADABLE, "invalid field status")
    return None


def snapshot(sample, interface):
    common = {"schema_version", "interface", "status"}
    check(type(sample) is dict and type(sample.get("schema_version")) is int
          and sample["schema_version"] == 1 and sample.get("interface") == interface,
          "sample does not match selected interface")
    if sample.get("status") != "observed":
        shape(sample, common, "invalid withheld sample")
        check(sample["status"] in (*UNREADABLE, "changed"), "invalid sample status")
        return None
    shape(sample, common | {"carrier", "speed_mbps", "duplex", "operstate", "counters"},
          "invalid observed sample")
    field(sample["carrier"], lambda v: type(v) is bool)
    field(sample["speed_mbps"], decimal)
    field(sample["duplex"], lambda v: v in ("half", "full"))
    field(sample["operstate"], lambda v: v in
          ("unknown", "notpresent", "down", "lowerlayerdown", "testing", "dormant", "up"))
    shape(sample["counters"], KEYS, "unexpected counter set")
    return {k: field(sample["counters"][k], decimal) for k in KEYS}


def verify(raw, interface, wait_ms):
    check(type(interface) is str and interface not in (".", "..")
          and re.fullmatch(r"[A-Za-z0-9_.-]{1,15}", interface), "invalid interface")
    check(type(wait_ms) is int and 1 <= wait_ms <= 60000, "invalid selected interval")
    report = parse(raw)
    shape(report, ("schema_version", "mode", "requested_wait_ms", "sample_start_interval_ms",
                   "continuity", "before", "after", "delta"), "invalid NIC window")
    check(type(report["schema_version"]) is int and report["schema_version"] == 1
          and report["mode"] == "two_passive_snapshots", "unsupported NIC window")
    check(type(report["requested_wait_ms"]) is int and report["requested_wait_ms"] == wait_ms
          and type(report["sample_start_interval_ms"]) is int
          and wait_ms <= report["sample_start_interval_ms"] <= U64_MAX, "invalid sample interval")
    before, after = (snapshot(report[k], interface) for k in ("before", "after"))
    continuity = report["continuity"]
    check(continuity in ("same_identity_observed", "identity_changed", "unknown"),
          "invalid continuity state")
    if before is None or after is None:
        check(continuity == "unknown", "withheld samples cannot assert continuity")
    expected = {"status": "not_comparable"}
    if continuity == "same_identity_observed":
        pairs = {k: (int(before[k]), int(after[k])) for k in KEYS
                 if before[k] is not None and after[k] is not None}
        if any(b < a for a, b in pairs.values()):
            expected = {"status": "counter_decreased_or_reset"}
        else:
            n = len(pairs)
            expected = {"status": "observed_differences", "comparable_counters": n,
                        "counter_coverage": "none" if not n else "complete" if n == len(KEYS) else "partial",
                        "counters": {k: ({"status": "observed",
                                          "value": str(pairs[k][1] - pairs[k][0])}
                                         if k in pairs else {"status": "endpoint_unavailable"})
                                     for k in KEYS}}
    check(equal(report["delta"], expected), "reported NIC delta disagrees with sample pair")
    return {"schema_version": 1, "kind": "offline_nic_window_check", "interface": interface,
            "evidence_sha256": hashlib.sha256(raw).hexdigest(), "delta": expected,
            "continuity_claim": continuity, "identity_attestation": "not_verified",
            "measurement_scope": "counter_arithmetic_only"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--evidence", required=True)
    cli.add_argument("--interface", required=True)
    cli.add_argument("--sample-ms", required=True, type=int)
    args = cli.parse_args()
    try:
        result = verify(read_bounded(args.evidence), args.interface, args.sample_ms)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Compare two saved IPv4 route lookups; do not touch the live network."""
import argparse
from ipaddress import IPv4Address
import json
import re

from check_hub_evidence import check, parse, read_bounded


def inspect(raw, interface, source, peer):
    check(re.fullmatch(r"[A-Za-z0-9_.-]{1,15}", interface)
          and interface not in (".", ".."), "invalid expected interface")
    source, peer = str(IPv4Address(source)), str(IPv4Address(peer))
    check(source != peer, "expected distinct route endpoints")
    # ip -j emits an array. Reuse the duplicate-key and strict JSON reader.
    rows = parse(b'{"routes":' + raw + b'}')["routes"]
    check(type(rows) is list and len(rows) == 1 and type(rows[0]) is dict,
          "expected exactly one IPv4 lookup result")
    row = rows[0]
    check(row.get("dst") == peer and row.get("from") == source,
          "lookup does not match expected source and destination")
    dev = row.get("dev")
    check(type(dev) is str and re.fullmatch(r"[A-Za-z0-9_.-]{1,15}", dev),
          "lookup has no valid interface")
    kind = row.get("type", "unicast")
    flags, cache = row.get("flags", []), row.get("cache", [])
    check(type(flags) is list and all(type(f) is str for f in flags)
          and type(cache) is list and all(type(f) is str for f in cache),
          "lookup has invalid route flags")
    if kind == "local" or dev == "lo" or "local" in flags or "local" in cache:
        status = "host_local"
    elif kind != "unicast":
        status = "unsupported_route_type"
    elif any(k in row for k in ("gateway", "via", "nexthops", "nhid", "encap", "multipath")):
        status = "routed_or_ambiguous"
    elif dev != interface:
        status = "interface_mismatch"
    else:
        status = "matches_direct_route_lookup"
    return {"status": status, "source": source, "destination": peer,
            "expected_interface": interface, "observed_interface": dev}


def correlate(sender, receiver, sender_if, sender_ip, receiver_if, receiver_ip):
    first = inspect(sender, sender_if, sender_ip, receiver_ip)
    second = inspect(receiver, receiver_if, receiver_ip, sender_ip)
    if any(row["status"] == "host_local" for row in (first, second)):
        status = "host_local_observed"
    elif all(row["status"] == "matches_direct_route_lookup" for row in (first, second)):
        status = "matched_route_lookups"
    else:
        status = "route_mismatch"
    return {"schema_version": 1, "kind": "isolated_peer_route_evidence",
            "status": status, "sender": first, "receiver": second,
            "actual_socket_path": "not_verified", "physical_path": "not_verified"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for role in ("sender", "receiver"):
        for field in ("route", "interface", "ip"):
            cli.add_argument(f"--{role}-{field}", required=True)
    a = cli.parse_args()
    try:
        result = correlate(read_bounded(a.sender_route), read_bounded(a.receiver_route),
                           a.sender_interface, a.sender_ip, a.receiver_interface, a.receiver_ip)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))
    return 0 if result["status"] == "matched_route_lookups" else 2


if __name__ == "__main__":
    raise SystemExit(main())

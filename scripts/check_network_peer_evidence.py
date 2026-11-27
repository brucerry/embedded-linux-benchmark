#!/usr/bin/env python3
"""Match two successful Engine peer captures against the stated test parameters."""
import argparse
import hashlib
from ipaddress import IPv4Address
import json
import re

from check_hub_evidence import check, equal, parse, read_bounded

PREFIX = b"Peer receipt: "


def record(raw):
    rows = [line[len(PREFIX):] for line in raw.splitlines() if line.startswith(PREFIX)]
    check(len(rows) == 1 and len(rows[0]) <= 1024, "expected exactly one bounded peer receipt")
    return parse(rows[0])


def correlate(sender, receiver, sender_ip, receiver_ip, port, session, mib, probes,
              sender_exit, receiver_exit):
    check(type(sender_exit) is int and type(receiver_exit) is int and
          sender_exit == receiver_exit == 0, "both captured processes must exit successfully")
    addr = tuple(str(IPv4Address(v)) for v in (sender_ip, receiver_ip))
    check(addr[0] != addr[1], "expected distinct endpoints")
    check(type(port) is int and 1 <= port <= 65535 and type(mib) is int and 1 <= mib <= 64
          and type(probes) is int and 1 <= probes <= 1000, "invalid peer test bounds")
    check(type(session) is str and re.fullmatch(r"[0-9a-f]{32}", session) and session != "0" * 32,
          "expected a fresh lowercase hexadecimal session label")
    common = {"schema_version": 1, "kind": "tcp_peer_result", "session": session,
              "port": port, "verified_bytes": mib * 1024**2, "probes": probes}
    check(equal(record(sender), {**common, "role": "sender", "bind": addr[0], "peer": addr[1]}),
          "sender receipt does not match the expected run")
    check(equal(record(receiver), {**common, "role": "receiver", "bind": addr[1], "peer": addr[0]}),
          "receiver receipt does not match the expected run")
    return {"schema_version": 1, "kind": "tcp_peer_evidence_match", "status": "correlated",
            "verified_bytes_per_endpoint": common["verified_bytes"], "probes": probes,
            "sender_sha256": hashlib.sha256(sender).hexdigest(),
            "receiver_sha256": hashlib.sha256(receiver).hexdigest(),
            "endpoint_authenticity": "not_verified", "physical_path": "not_verified"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for name in ("sender-output", "receiver-output", "sender-ip", "receiver-ip", "session"):
        cli.add_argument("--" + name, required=True)
    for name in ("port", "size-mib", "probes", "sender-exit", "receiver-exit"):
        cli.add_argument("--" + name, type=int, required=True)
    a = cli.parse_args()
    try:
        result = correlate(read_bounded(a.sender_output), read_bounded(a.receiver_output),
                           a.sender_ip, a.receiver_ip, a.port, a.session, a.size_mib, a.probes,
                           a.sender_exit, a.receiver_exit)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

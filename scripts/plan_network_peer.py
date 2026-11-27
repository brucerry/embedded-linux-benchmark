#!/usr/bin/env python3
"""Preview a bounded Engine direct-link test; never launch it."""
import argparse
from ipaddress import IPv4Interface, IPv4Network
import json
import re

RFC1918 = tuple(IPv4Network(x) for x in ("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"))


def endpoint(interface, address):
    if not re.fullmatch(r"[A-Za-z0-9_.-]{1,15}", interface) or interface in (".", "..", "lo"):
        raise ValueError("expected the named non-loopback lab interface")
    ip = IPv4Interface(address)
    if not any(ip.network.subnet_of(n) for n in RFC1918) or ip.ip.packed[-1] in (0, 255):
        raise ValueError("expected an RFC1918 lab host address")
    if not 16 <= ip.network.prefixlen <= 31:
        raise ValueError("expected a lab prefix from /16 through /31")
    if ip.network.prefixlen < 31 and ip.ip in (ip.network.network_address, ip.network.broadcast_address):
        raise ValueError("network and broadcast addresses are not endpoints")
    return {"interface": interface, "address": str(ip.ip)}, ip


def plan(sender_if, sender_address, receiver_if, receiver_address, port, session, mib, pings, timeout):
    sender, a = endpoint(sender_if, sender_address)
    receiver, b = endpoint(receiver_if, receiver_address)
    if a.network != b.network or a.ip == b.ip:
        raise ValueError("endpoints must have distinct addresses on the same subnet")
    for value, low, high, label in ((port, 1024, 65535, "port"), (mib, 1, 64, "payload"),
                                     (pings, 1, 1000, "pings"), (timeout, 1, 30000, "timeout")):
        if type(value) is not int or not low <= value <= high:
            raise ValueError(f"{label} outside the supported range")
    if not re.fullmatch(r"[0-9a-f]{32}", session) or session == "0" * 32:
        raise ValueError("expected a nonzero lowercase 32-hex session label")
    flags = ["--port", str(port), "--session", session, "--size-mib", str(mib),
             "--pings", str(pings), "--timeout-ms", str(timeout)]

    def qualify(local, remote, mode):
        return {**local, "peer": remote["address"],
                "health_argv": ["eld-engine", "network", "health", "--interface", local["interface"]],
                "peer_argv": ["eld-engine", "network", mode, "--bind", local["address"],
                              "--peer", remote["address"], *flags]}

    return {"schema_version": 1, "kind": "isolated_tcp_peer_preview",
            "status": "preview_only", "execution": "none", "path_verified": False,
            "physical_measurement_verified": False,
            "expected_payload_bytes": mib * 1024 * 1024, "expected_round_trips": pings,
            "endpoint_deadline_ms": timeout, "subnet": str(a.network),
            "receiver": qualify(receiver, sender, "peer-server"),
            "sender": qualify(sender, receiver, "peer-client")}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--sender", nargs=2, required=True, metavar=("INTERFACE", "IP/PREFIX"))
    cli.add_argument("--receiver", nargs=2, required=True, metavar=("INTERFACE", "IP/PREFIX"))
    cli.add_argument("--port", type=int, required=True)
    cli.add_argument("--session", required=True)
    cli.add_argument("--size-mib", type=int, default=1)
    cli.add_argument("--pings", type=int, default=20)
    cli.add_argument("--timeout-ms", type=int, default=5000)
    args = cli.parse_args()
    try:
        result = plan(*args.sender, *args.receiver, args.port, args.session, args.size_mib,
                      args.pings, args.timeout_ms)
    except ValueError as exc:
        cli.exit(1, f"Plan rejected: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

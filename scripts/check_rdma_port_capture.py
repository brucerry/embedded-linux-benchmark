#!/usr/bin/env python3
"""Decode a saved copy of four Linux RDMA port attributes. No device access."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded

CAPTURE_LIMIT = 4096
FIELDS = {"state", "phys_state", "link_layer", "rate"}
STATE = re.compile(r"(0|[1-9][0-9]{0,2}): ([A-Za-z0-9_ <>-]{1,40})")
RATE = re.compile(r"(0|[1-9][0-9]{0,5})(\.5)? Gb/sec \(([1-9][0-9]?)X ([A-Z0-9]{1,12})\)")


def decode(raw):
    check(len(raw) <= CAPTURE_LIMIT, "RDMA capture exceeds byte limit")
    value = parse(raw)
    check(type(value) is dict and value.keys() == FIELDS, "expected exactly four saved RDMA attributes")
    fields = {}
    for key in sorted(FIELDS):
        value_raw = value[key]
        if value_raw is None:
            fields[key] = {"status": "unavailable"}
            continue
        check(type(value_raw) is str and 0 < len(value_raw) <= 64 and value_raw.isascii(),
              f"invalid saved RDMA {key}")
        text = value_raw.removesuffix("\n")
        sample = {"status": "reported", "raw": text}
        if key in ("state", "phys_state"):
            match = STATE.fullmatch(text)
            check(match and int(match[1]) <= 255, f"invalid saved RDMA {key}")
            sample.update(code=match[1], kernel_label=match[2])
        elif key == "link_layer":
            check(text in ("InfiniBand", "Ethernet", "Unknown"), "invalid saved RDMA link_layer")
        else:
            match = RATE.fullmatch(text)
            check(match, "invalid saved RDMA rate")
            sample.update(gigabits_per_second=match[1] + (match[2] or ""),
                          reported_width=match[3], kernel_mode=match[4])
        fields[key] = sample
    return {"schema_version": 1, "kind": "rdma_port_capture_decode", "source": "provided_capture",
            "hardware_authenticity": "not_verified", "port_attribution": "not_present",
            "device_sampled": False, "throughput_measured": False,
            "capture_sha256": hashlib.sha256(raw).hexdigest(), "attributes": fields}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--capture", required=True, help="saved JSON fixture; null for unavailable attributes")
    args = cli.parse_args()
    try:
        result = decode(read_bounded(args.capture))
    except (OSError, ValueError, RecursionError) as e:
        cli.exit(1, f"RDMA capture refused: {e}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Check one saved Engine RDMA port report against an explicit selection."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, parse, read_bounded
from check_rdma_port_capture import FIELDS, CAPTURE_LIMIT

NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,62}")
STATUSES = {"unavailable", "inaccessible", "invalid"}
SCOPE = {"schema_version": 1, "kind": "passive_rdma_port", "source": "linux_sysfs",
         "rate_scope": "kernel_reported_rdma", "ethernet_line_rate": "not_assessed",
         "hardware_provenance": "not_assessed", "throughput_measured": False,
         "traffic_started": False}


def verify(raw, device, port, expected_sha256=None):
    check(type(device) is str and NAME.fullmatch(device)
          and type(port) is int and 0 <= port <= 2**32 - 1, "invalid expected port selection")
    check(len(raw) <= CAPTURE_LIMIT, "RDMA report exceeds byte limit")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(digest == expected_sha256, "report differs from expected SHA-256")
    x = parse(raw)
    check(type(x) is dict and x.keys() == SCOPE.keys() | {"selection", "status", "attributes"},
          "invalid RDMA report shape")
    check(all(type(x[k]) is type(v) and x[k] == v for k, v in SCOPE.items()),
          "invalid RDMA report scope")
    sel = x["selection"]
    check(type(sel) is dict and sel.keys() == {"device", "port"} and type(sel["port"]) is int
          and sel == {"device": device, "port": port}, "RDMA report differs from expected selection")
    status = x["status"]
    attributes = x["attributes"]
    check(type(status) is str and type(attributes) is dict, "invalid RDMA status or attributes")
    if status == "observed":
        check(attributes.keys() == FIELDS, "missing observed RDMA attributes")
        for key, v in attributes.items():
            check(type(v) is dict and type(v.get("status")) is str, f"invalid RDMA {key}")
            if v["status"] == "reported":
                text = v.get("raw")
                check(v.keys() == {"status", "raw"} and type(text) is str
                      and 0 < len(text) <= 64 and all(32 <= ord(c) < 127 for c in text)
                      and text == text.strip(), f"invalid RDMA {key} raw value")
            else:
                check(v.keys() == {"status"} and v["status"] in STATUSES,
                      f"invalid withheld RDMA {key}")
    else:
        check(status in STATUSES and not attributes, "invalid withheld RDMA report")
    return {"schema_version": 1, "kind": "passive_rdma_report_check",
            "status": status, "selection": sel, "attributes": attributes,
            "report_sha256": digest,
            "digest_selection": "matched" if expected_sha256 is not None else "unbound",
            "hardware_authenticity": "not_verified", "ethernet_line_rate": "not_assessed",
            "throughput_measured": False}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--report", required=True)
    cli.add_argument("--device", required=True)
    cli.add_argument("--port", type=int, required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        out = verify(read_bounded(args.report), args.device, args.port, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as e:
        cli.exit(1, f"RDMA report refused: {e}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

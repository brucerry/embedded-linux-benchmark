#!/usr/bin/env python3
"""Offline, bounded example of per-host PCIe expectation summaries."""
import argparse
import json
import re

from check_hub_evidence import check, parse, read_bounded, verify

MAX_RUNS = 8
LABEL = re.compile("[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}")
BDF = re.compile("[0-9a-f]{4}:[0-9a-f]{2}:[0-1][0-9a-f]\\.[0-7]")
STATES = ("present", "match", "missing", "mismatch", "inaccessible",
          "invalid", "unavailable", "changed")


def summarize(entries):
    check(1 <= len(entries) <= MAX_RUNS, "expected 1 to 8 evidence pairs")
    out = []
    seen_hosts, seen_ids, seen_digests = set(), set(), set()
    for host, token, evidence, stdout in entries:
        check(LABEL.fullmatch(host) and host not in seen_hosts, "invalid or repeated host label")
        receipt = verify(evidence, stdout, token)
        digest = receipt["sha256"]
        check(token not in seen_ids and digest not in seen_digests, "repeated run or evidence")
        seen_hosts.add(host)
        seen_ids.add(token)
        seen_digests.add(digest)
        inventory = parse(stdout)["inventory"]
        rows = inventory.get("required_pcie", [])
        check(type(rows) is list and len(rows) <= 16, "invalid PCIe expectation rows")
        counts = dict.fromkeys(STATES, 0)
        devices = set()
        for row in rows:
            check(type(row) is dict, "invalid PCIe expectation row")
            bdf, state = row.get("bdf"), row.get("status")
            check(type(bdf) is str and BDF.fullmatch(bdf) and bdf not in devices
                  and type(state) is str and state in STATES, "invalid or duplicate PCIe row")
            devices.add(bdf)
            counts[state] += 1
        out.append({
            "host_label": host, "run_id": token, "sha256": digest,
            "status": "summarized" if rows else "no_expectation_rows",
            "counts": counts,
        })
    return {"schema_version": 1, "kind": "offline_pcie_expectation_summary",
            "source_identity": "not_verified", "records_by_host": out}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--run", action="append", nargs=4, required=True,
                     metavar=("HOST", "RUN_ID", "EVIDENCE", "STDOUT"))
    args = cli.parse_args()
    try:
        check(len(args.run) <= MAX_RUNS, "expected 1 to 8 evidence pairs")
        entries = [(h, r, read_bounded(e), read_bounded(o)) for h, r, e, o in args.run]
        out = summarize(entries)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

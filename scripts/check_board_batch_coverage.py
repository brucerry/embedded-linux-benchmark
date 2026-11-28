#!/usr/bin/env python3
"""Compare verified saved PCIe board runs with an independently selected roster."""
import argparse
import hashlib
import json
from pathlib import Path

from check_hub_evidence import check, parse, read_bounded
from summarize_board_pcie import LABEL, summarize, valid_digest

OUTCOMES = ("pass", "fail", "unavailable", "not_assessed", "no_capture")


def verify(manifest_raw, directory, roster_raw, expected_roster_sha256):
    check(type(roster_raw) is bytes and 0 < len(roster_raw) <= 8192, "invalid roster size")
    roster_hash = hashlib.sha256(roster_raw).hexdigest()
    check(valid_digest(expected_roster_sha256) and expected_roster_sha256 == roster_hash,
          "roster differs from the selected digest")
    roster = parse(roster_raw)
    check(type(roster) is dict and roster.keys() == {"schema_version", "boards"}
          and type(roster["schema_version"]) is int and roster["schema_version"] == 1
          and type(roster["boards"]) is list and 1 <= len(roster["boards"]) <= 16,
          "invalid expected board roster")
    expected, seen = {}, set()
    for board in roster["boards"]:
        check(type(board) is dict and board.keys() == {"board", "profile_sha256", "engine_sha256"}
              and type(board["board"]) is str and LABEL.fullmatch(board["board"])
              and all(valid_digest(board[key]) for key in ("profile_sha256", "engine_sha256")),
              "invalid expected board")
        name = board["board"]
        check(name.casefold() not in seen, "duplicate expected board")
        seen.add(name.casefold())
        expected[name] = board
    manifest = parse(manifest_raw)
    check(type(manifest) is dict and manifest.keys() == {"schema_version", "boards"}
          and type(manifest["schema_version"]) is int and manifest["schema_version"] == 2
          and type(manifest["boards"]) is list and len(manifest["boards"]) <= 16,
          "require v2 capture manifest")
    if manifest["boards"]:
        summary = summarize(manifest_raw, directory)
        rows = {row["board"]: row for row in summary["boards"]}
    else:
        rows = {}
    check(rows.keys() <= expected.keys(), "capture includes a board not in the roster")
    counts = dict.fromkeys(OUTCOMES, 0)
    result = []
    for name, board in expected.items():
        status = "no_capture"
        run = None
        if name in rows:
            row = rows[name]
            check(all(row["context"][key] == board[key] for key in ("profile_sha256", "engine_sha256")),
                  "captured profile or Engine differs from expected board")
            status = row["result"]["outcome"]
            run = row["result"]["run_id"]
        counts[status] += 1
        result.append({"board": name, "evidence_outcome": status, "run_id": run})
    return {"schema_version": 1, "kind": "offline_pcie_roster_coverage", "counts": counts,
            "boards": result, "roster_sha256": roster_hash,
            "manifest_sha256": hashlib.sha256(manifest_raw).hexdigest(),
            "hardware_identity": "not_verified", "manufacturing_yield": "not_assessed"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--manifest", required=True, type=Path)
    cli.add_argument("--roster", required=True, type=Path)
    cli.add_argument("--expected-roster-sha256", required=True)
    args = cli.parse_args()
    try:
        out = verify(read_bounded(args.manifest), args.manifest.parent,
                     read_bounded(args.roster), args.expected_roster_sha256)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Board coverage evidence rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

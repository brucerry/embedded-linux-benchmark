#!/usr/bin/env python3
"""Summarize a bounded batch of offline PCIe checks, one result per selected board."""
import argparse
import hashlib
import json
from pathlib import Path
import re

from check_hub_evidence import RUN_ID, check, parse, pcie_expectations, read_bounded
from evaluate_pcie_requirements import evaluate

LABEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,63}", re.ASCII)
DIGEST = re.compile(r"[0-9a-f]{64}", re.ASCII)
FILES = ("evidence", "stdout", "plan")
V2_FILES = FILES + ("profile", "capture")


def valid_digest(value):
    return type(value) is str and DIGEST.fullmatch(value) is not None


def verify_context(artifacts, board, run_id, expected_engine, process_exit):
    profile = parse(artifacts["profile"])
    check(profile.keys() == {"schema_version", "profile_id", "require_pcie"}
          and type(profile["schema_version"]) is int and profile["schema_version"] == 1
          and type(profile["profile_id"]) is str and LABEL.fullmatch(profile["profile_id"]),
          "invalid PCIe board profile")
    required = pcie_expectations(profile["require_pcie"])
    argv = parse(artifacts["plan"]).get("argv")
    check(type(argv) is list and len(argv) >= 8 and argv[6] == "--require-pcie"
          and pcie_expectations(argv[7]) == required, "PCIe plan differs from board profile")
    c = parse(artifacts["capture"])
    check(c.keys() == {"schema_version", "board", "run_id", "engine_sha256", "process_exit"}
          and type(c["schema_version"]) is int and c["schema_version"] == 1
          and c["board"] == board and c["run_id"] == run_id
          and type(c["process_exit"]) is int and c["process_exit"] == process_exit,
          "capture context differs from manifest")
    check(valid_digest(c["engine_sha256"]) and c["engine_sha256"] == expected_engine,
          "captured Engine digest differs from expected asset")
    return {"profile_id": profile["profile_id"],
            "profile_sha256": hashlib.sha256(artifacts["profile"]).hexdigest(),
            "engine_sha256": expected_engine,
            "capture_sha256": hashlib.sha256(artifacts["capture"]).hexdigest(),
            "binary_binding": "controller_record_matches_expected_asset",
            "runtime_attestation": "not_verified"}


def summarize(manifest_raw, directory):
    manifest = parse(manifest_raw)
    check(manifest.keys() == {"schema_version", "boards"}
          and type(manifest["schema_version"]) is int and manifest["schema_version"] in (1, 2),
          "unsupported board manifest")
    version = manifest["schema_version"]
    files = V2_FILES if version == 2 else FILES
    fields = {"board", "run_id", "process_exit", "sha256"}
    if version == 2:
        fields.add("expected_engine_sha256")
    boards = manifest["boards"]
    check(type(boards) is list and 1 <= len(boards) <= 16, "expected 1 to 16 boards")
    labels, run_ids, evidence_hashes = set(), set(), set()
    results, groups, unbound = [], {}, []
    for row in boards:
        check(type(row) is dict and row.keys() == fields,
              "invalid board record")
        board, run_id, digests = row["board"], row["run_id"], row["sha256"]
        check(type(board) is str and LABEL.fullmatch(board) and board.casefold() not in labels,
              "invalid or repeated board label")
        check(type(run_id) is str and RUN_ID.fullmatch(run_id) and run_id not in run_ids,
              "invalid or repeated run ID")
        check(type(digests) is dict and digests.keys() == set(files)
              and all(valid_digest(digest) for digest in digests.values()),
              "invalid artifact digests")
        check(digests["evidence"] not in evidence_hashes, "repeated evidence digest")
        labels.add(board.casefold())
        run_ids.add(run_id)
        evidence_hashes.add(digests["evidence"])
        artifacts = {}
        for kind in files:
            data = read_bounded(directory / f"{run_id}.{kind}.json")
            check(hashlib.sha256(data).hexdigest() == digests[kind], f"{board}: {kind} digest mismatch")
            artifacts[kind] = data
        result = evaluate(artifacts["evidence"], artifacts["stdout"], run_id,
                          artifacts["plan"], row["process_exit"])
        entry = {"board": board, "result": result}
        if version == 2:
            expected_engine = row["expected_engine_sha256"]
            check(valid_digest(expected_engine), "invalid expected Engine digest")
            entry["context"] = verify_context(artifacts, board, run_id, expected_engine,
                                              row["process_exit"])
            context = entry["context"]
            key = (context["profile_sha256"], context["engine_sha256"])
            group = groups.setdefault(key, {
                "profile_id": context["profile_id"], "profile_sha256": key[0],
                "engine_sha256": key[1], "boards": [],
            })
            group["boards"].append(board)
        else:
            unbound.append(board)
        results.append(entry)
    return {"schema_version": version, "kind": "offline_board_pcie_summary",
            "scope": "selected_board_runs_only",
            "manifest_sha256": hashlib.sha256(manifest_raw).hexdigest(),
            "boards": results,
            "comparison": {"scope": "exact_profile_bytes_and_engine_digest_only",
                           "groups": [groups[key] for key in sorted(groups)],
                           "unbound_boards": unbound},
            "hardware_authenticity": "not_verified",
            "manufacturing_yield": "not_assessed"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--manifest", type=Path, required=True,
                     help="manifest in the same private directory as the raw artifacts")
    args = cli.parse_args()
    try:
        print(json.dumps(summarize(read_bounded(args.manifest), args.manifest.parent), sort_keys=True))
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Batch rejected: {exc}\n")


if __name__ == "__main__":
    main()

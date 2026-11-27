#!/usr/bin/env python3
"""Evaluate explicitly planned passive PCIe requirements, never certify a device."""
import argparse
import hashlib
import json

from check_hub_evidence import check, parse, read_bounded, verify


def evaluate(evidence, stdout, run_id, plan_raw, process_exit):
    check(type(process_exit) is int and process_exit == 0,
          "require the successful exit status of the exact passive invocation")
    correlation = verify(evidence, stdout, run_id, parse(plan_raw))
    rows = parse(stdout)["inventory"].get("required_pcie", [])
    counts = {"pass": 0, "fail": 0, "unavailable": 0}
    results = []
    for row in rows:
        code = row["status"]
        outcome = ("pass" if code in ("present", "match") else
                   "fail" if code in ("missing", "mismatch") else "unavailable")
        counts[outcome] += 1
        results.append({"bdf": row["bdf"], "outcome": outcome, "observation": code,
                        "requirement": "device_identity" if "expected" in row else "presence_only"})
    overall = ("not_assessed" if not rows else "fail" if counts["fail"] else
               "unavailable" if counts["unavailable"] else "pass")
    return {"schema_version": 1, "kind": "passive_pcie_requirements",
            "run_id": run_id, "scope": "explicit_pcie_requirements_only",
            "outcome": overall, "counts": counts, "rows": results,
            "evidence_sha256": correlation["sha256"], "plan_sha256": hashlib.sha256(plan_raw).hexdigest(),
            "hardware_authenticity": "not_verified", "production_qualification": "not_assessed"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    for key in ("evidence", "stdout", "run-id", "plan"):
        cli.add_argument("--" + key, required=True)
    cli.add_argument("--process-exit", type=int, required=True)
    args = cli.parse_args()
    try:
        out = evaluate(read_bounded(args.evidence), read_bounded(args.stdout), args.run_id,
                       read_bounded(args.plan), args.process_exit)
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

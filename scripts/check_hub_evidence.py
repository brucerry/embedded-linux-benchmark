#!/usr/bin/env python3
"""Reference verifier for Hub evidence handoffs, not a deployer or hardware probe."""
import argparse
import hashlib
import json
import os
from decimal import Decimal
import re
import stat
import sys

MAX_BYTES = 1024 * 1024
RUN_ID = re.compile("[0-9a-f]{32}")


def check(condition, reason):
    if not condition:
        raise ValueError(reason)


def read_bounded(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        before = os.fstat(stream.fileno())
        check(stat.S_ISREG(before.st_mode) and 0 < before.st_size <= MAX_BYTES,
              "expected a bounded nonempty regular file")
        data = stream.read(MAX_BYTES + 1)
        after = os.fstat(stream.fileno())
        check(len(data) == before.st_size == after.st_size
              and before.st_mtime_ns == after.st_mtime_ns
              and before.st_ctime_ns == after.st_ctime_ns, "file changed while reading")
        return data


def parse(data):
    def unique(items):
        out = {}
        for key, value in items:
            check(key not in out, "duplicate JSON key")
            out[key] = value
        return out

    def bad_constant(_):
        raise ValueError("non-finite JSON number")

    check(0 < len(data) <= MAX_BYTES, "JSON byte limit exceeded")
    out = json.loads(data.decode("utf-8"), object_pairs_hook=unique,
                     parse_float=Decimal, parse_constant=bad_constant)
    check(type(out) is dict, "expected a JSON object")
    return out


def equal(x, y):
    # A bool, integer and decimal must not compare equal across JSON types.
    if type(x) is not type(y):
        return False
    if isinstance(x, dict):
        return x.keys() == y.keys() and all(equal(x[k], y[k]) for k in x)
    if isinstance(x, list):
        return len(x) == len(y) and all(equal(a, b) for a, b in zip(x, y))
    return x == y


def verify_pressure_series(series, elapsed):
    expected = {"schema_version", "kind", "scope", "sample_count",
                "requested_gap_ms", "workload_execution", "samples"}
    check(type(series) is dict and series.keys() == expected
          and type(series["schema_version"]) is int and series["schema_version"] == 1
          and series["kind"] == "linux_pressure_series" and series["scope"] == "single_process"
          and series["workload_execution"] == "none", "unsupported pressure series")
    count, gap, rows = (series[k] for k in ("sample_count", "requested_gap_ms", "samples"))
    check(type(count) is int and 2 <= count <= 8 and type(gap) is int and 100 <= gap <= 1000
          and type(rows) is list and len(rows) == count, "invalid pressure series bounds")
    previous = None
    for index, row in enumerate(rows):
        check(type(row) is dict and row.keys() == {
            "index", "started_monotonic_ms", "finished_monotonic_ms", "pressure"
        } and type(row["index"]) is int and row["index"] == index, "invalid pressure sample")
        start, finish = row["started_monotonic_ms"], row["finished_monotonic_ms"]
        check(type(start) is int and type(finish) is int and 0 <= start <= finish <= elapsed
              and (previous is None or start - previous >= gap), "invalid pressure timing")
        pressure = row["pressure"]
        check(type(pressure) is dict and pressure.keys() == {
            "schema_version", "scope", "cpu", "memory", "io"
        } and type(pressure["schema_version"]) is int and pressure["schema_version"] == 1
            and pressure["scope"] == "system_proc_pressure"
            and all(type(pressure[k]) is dict for k in ("cpu", "memory", "io")),
            "invalid pressure snapshot envelope")
        previous = finish


def verify_network_window(window, elapsed):
    check(window.keys() == {"schema_version", "mode", "requested_wait_ms",
                            "sample_start_interval_ms", "continuity", "before", "after", "delta"}
          and type(window["schema_version"]) is int and window["schema_version"] == 1
          and window["mode"] == "two_passive_snapshots", "unsupported network window")
    wait, interval = window["requested_wait_ms"], window["sample_start_interval_ms"]
    check(type(wait) is int and type(interval) is int and 1 <= wait <= 2000
          and wait <= interval <= elapsed, "invalid network sample timing")
    a, b, delta = (window[k] for k in ("before", "after", "delta"))
    check(all(type(row) is dict for row in (a, b, delta))
          and type(a.get("interface")) is str and a.get("interface") == b.get("interface"),
          "network snapshot interface mismatch")
    continuity = window["continuity"]
    check(continuity in ("same_identity_observed", "identity_changed", "unknown"),
          "invalid network continuity")
    status = delta.get("status")
    if continuity == "same_identity_observed":
        check(a.get("status") == b.get("status") == "observed"
              and status in ("observed_differences", "counter_decreased_or_reset"),
              "invalid network delta for matched identity")
    else:
        check(delta == {"status": "not_comparable"}, "network delta without continuity")
    if status == "observed_differences":
        check(type(delta.get("counters")) is dict, "missing network counters")
    else:
        check(delta.keys() == {"status"}, "invalid withheld network delta")


def verify(evidence, stdout, run_id):
    check(isinstance(run_id, str) and RUN_ID.fullmatch(run_id)
          and run_id != "0" * 32, "invalid expected run ID")
    saved = parse(evidence)
    expected = {"evidence_schema_version", "kind", "started_unix_ms",
                "finished_unix_ms", "elapsed_monotonic_ms", "qualification"}
    check(saved.keys() == expected and type(saved["evidence_schema_version"]) is int
          and saved["evidence_schema_version"] == 1
          and saved["kind"] == "passive_qualification", "unsupported evidence envelope")
    for field in ("started_unix_ms", "finished_unix_ms", "elapsed_monotonic_ms"):
        check(type(saved[field]) is int and saved[field] >= 0, "invalid timestamp or duration")
    q = parse(stdout)
    check(type(q.get("qualification_schema_version")) is int
          and q["qualification_schema_version"] == 1
          and q.get("kind") == "passive_qualification_observation"
          and q.get("scope") == "bring-up" and q.get("workload_execution") == "none"
          and type(q.get("inventory")) is dict, "unsupported passive observation")
    check(q.get("run_id") == run_id, "run ID mismatch")
    check(equal(saved["qualification"], q), "saved qualification differs from command output")
    health = q.get("system_health")
    if type(health) is dict and "pressure_series" in health:
        verify_pressure_series(health["pressure_series"], saved["elapsed_monotonic_ms"])
    network = q.get("network_health")
    if type(network) is dict and "mode" in network:
        verify_network_window(network, saved["elapsed_monotonic_ms"])
    return {"status": "correlated", "run_id": run_id,
            "evidence_bytes": len(evidence), "sha256": hashlib.sha256(evidence).hexdigest(),
            "hardware_authenticity": "not_verified"}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--evidence", required=True)
    p.add_argument("--stdout", required=True)
    p.add_argument("--run-id", required=True)
    a = p.parse_args()
    try:
        receipt = verify(read_bounded(a.evidence), read_bounded(a.stdout), a.run_id)
    except (OSError, ValueError, RecursionError) as exc:
        p.exit(1, f"Evidence rejected: {exc}\n")
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()

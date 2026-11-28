#!/usr/bin/env python3
"""Find known root-port AER notice lines in a saved kernel log."""
import argparse
import hashlib
import json
import re

from check_hub_evidence import check, read_bounded

BDF = r"[0-9a-f]{4}:[0-9a-f]{2}:[01][0-9a-f]\.[0-7]"
SEVERITY = {"Corrected": "corrected", "Uncorrected (Non-Fatal)": "nonfatal",
            "Uncorrected (Fatal)": "fatal"}
LINE = re.compile(
    rf"(?:\[ *[0-9]{{1,16}}(?:\.[0-9]{{1,9}})?\] +)?pcieport ({BDF}): (?:AER: )?"
    rf"(Multiple )?(Corrected|Uncorrected \(Non-Fatal\)|Uncorrected \(Fatal\))"
    rf" error received: ({BDF})"
)
MAX_LINES = 8192
MAX_MATCHES = 256


def verify(raw, observer, expected_sha256=None):
    check(type(observer) is str and re.fullmatch(BDF, observer), "invalid expected observer BDF")
    digest = hashlib.sha256(raw).hexdigest()
    if expected_sha256 is not None:
        check(type(expected_sha256) is str and re.fullmatch(r"[0-9a-f]{64}", expected_sha256),
              "expected a lowercase SHA-256 digest")
        check(digest == expected_sha256, "log differs from expected SHA-256")
    lines = raw.decode("utf-8").split("\n")
    check(len(lines) <= MAX_LINES, "too many saved log lines")
    matches = []
    omitted = 0
    for n, text in enumerate(lines, 1):
        match = LINE.fullmatch(text.removesuffix("\r")) if len(text) <= 256 else None
        if not match or match[1] != observer:
            continue
        if len(matches) == MAX_MATCHES:
            omitted += 1
            continue
        matches.append({"line": n, "observer_bdf": match[1], "source_id_printed": match[4],
                        "severity": SEVERITY[match[3]], "multiple_flag": match[2] is not None})
    return {"schema_version": 1, "kind": "offline_pcie_aer_log_check",
            "status": "matching_notices" if matches else "no_matching_notices",
            "observer_bdf": observer, "log_sha256": digest, "omitted_matching_notices": omitted,
            "digest_selection": "matched" if expected_sha256 is not None else "unbound",
            "notices": matches, "hardware_errors_counted": False, "root_cause": "not_assessed",
            "live_query": "not_performed"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--log", required=True)
    cli.add_argument("--observer-bdf", required=True)
    cli.add_argument("--expected-sha256")
    args = cli.parse_args()
    try:
        out = verify(read_bounded(args.log), args.observer_bdf, args.expected_sha256)
    except (OSError, ValueError, RecursionError) as e:
        cli.exit(1, f"AER log refused: {e}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

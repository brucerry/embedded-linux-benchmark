"""Exercise the shipped NVMe decoder and the host-side receipt with inert bytes."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"


def smoke(engine):
    page = bytearray(512)
    page[:6] = bytes([2, 44, 1, 255, 10, 123])
    page[48:64] = (2**64 + 3).to_bytes(16, "little")
    page[160:176] = (2**128 - 1).to_bytes(16, "little")
    digest = hashlib.sha256(page).hexdigest()
    decoded = subprocess.run([engine, "storage", "nvme-smart-decode"],
                             input=bytes(page), capture_output=True, timeout=5, check=True).stdout
    with tempfile.TemporaryDirectory(prefix="eld-nvme-evidence-") as directory:
        raw = Path(directory) / "capture.bin"
        report = Path(directory) / "decoded.json"
        raw.write_bytes(page)
        report.write_bytes(decoded)
        argv = [sys.executable, "-B", str(SCRIPTS / "check_nvme_smart_evidence.py"),
                "--page", str(raw), "--report", str(report), "--expected-sha256", digest]
        match = subprocess.run(argv, capture_output=True, check=True, timeout=5)
        result = json.loads(match.stdout)
        if (result.get("status"), result.get("capture_provenance"), result.get("drive_health")) != (
                "correlated", "not_verified", "not_assessed"):
            raise ValueError("unexpected receipt or unverified hardware claim")
        tampered = json.loads(decoded)
        tampered["media_errors_raw"] = "0"
        report.write_text(json.dumps(tampered), encoding="utf-8")
        refusal = subprocess.run(argv, capture_output=True, timeout=5)
        if refusal.returncode != 1 or refusal.stdout or b"does not match" not in refusal.stderr:
            raise ValueError("receipt verifier accepted a tampered decoded counter")
        report.write_bytes(decoded)
        later = bytearray(page)
        later[48:64] = (2**112 + 2**64 + 3).to_bytes(16, "little")
        later[160:176] = (2**128 - 2).to_bytes(16, "little")
        later_page = Path(directory) / "later.bin"
        later_report = Path(directory) / "later.json"
        later_page.write_bytes(later)
        later_decoded = subprocess.run(
            [engine, "storage", "nvme-smart-decode"], input=bytes(later),
            capture_output=True, check=True, timeout=5,
        ).stdout
        later_report.write_bytes(later_decoded)
        pair = [sys.executable, "-B", str(SCRIPTS / "compare_nvme_smart_evidence.py"),
                "--before", str(raw), str(report), digest,
                "--after", str(later_page), str(later_report), hashlib.sha256(later).hexdigest()]
        comparison = json.loads(subprocess.run(
            pair, capture_output=True, check=True, timeout=5,
        ).stdout)
        expected = {
            "data_units_written": {"status": "observed", "value": str(2**112)},
            "data_units_read": {"status": "not_comparable", "reason": "unreported_endpoint"},
            "media_errors_raw": {"status": "decreased_or_reset"},
            "unsafe_shutdowns_raw": {"status": "observed", "value": "0"},
        }
        if (comparison["device_sampled"] is not False
                or comparison["capture_identity"] != "not_verified"
                or any(comparison["counters"][key] != value for key, value in expected.items())):
            raise ValueError("unexpected offline comparison semantics")
        later_report.write_bytes(decoded)
        refusal = subprocess.run(pair, capture_output=True, timeout=5)
        if refusal.returncode != 1 or refusal.stdout or b"does not match" not in refusal.stderr:
            raise ValueError("comparison accepted a report bound to the wrong page")
    print("Synthetic NVMe decode, evidence correlation and counter comparison passed")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("engine", type=Path, help="trusted locally built Engine executable")
    smoke(str(cli.parse_args().engine.resolve(strict=True)))

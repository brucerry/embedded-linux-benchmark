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
    print("Synthetic NVMe CLI evidence correlation and decoded-counter refusal passed")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("engine", type=Path, help="trusted locally built Engine executable")
    smoke(str(cli.parse_args().engine.resolve(strict=True)))

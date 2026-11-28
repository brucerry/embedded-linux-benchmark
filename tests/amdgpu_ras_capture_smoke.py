#!/usr/bin/env python3
"""Offline smoke: advertised Engine CLI output through the saved-report checker."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def run(*args):
    return subprocess.run(args, capture_output=True, timeout=15, check=False)


def main():
    engine = str(Path(sys.argv[1]).resolve(strict=True))
    checker = str(Path(__file__).resolve().parents[1] / "scripts/check_amdgpu_ras_evidence.py")
    command = (engine, "accelerator", "amdgpu-ras-decode", "--capture-text")
    capabilities = run(engine, "capabilities")
    capabilities.check_returncode()
    contract = json.loads(capabilities.stdout)["contracts"]["offline_amdgpu_ras"]
    assert contract == {"schema": 1, "input": "literal_capture_text", "max_bytes": 128}
    with tempfile.TemporaryDirectory() as temp:
        path = Path(temp) / "ras.json"
        cases = (
            ("ce: 18446744073709551615\nue: 0", "decoded",
             {"corrected_errors": "18446744073709551615", "uncorrected_errors": "0"}),
            ("Query currently inaccessible", "unavailable", {}),
        )
        for capture, status, counters in cases:
            decoded = run(*command, capture)
            decoded.check_returncode()
            path.write_bytes(decoded.stdout)
            digest = hashlib.sha256(decoded.stdout).hexdigest()
            check = (sys.executable, "-B", checker, "--report", str(path),
                     "--expected-sha256", digest)
            result = run(*check)
            result.check_returncode()
            output = json.loads(result.stdout)
            assert output["status"] == status and output["counters"] == counters
            assert output["digest_selection"] == "matched"
            assert output["device_attribution"] == "not_present"
            assert output["continuity"] == "not_assessed" and output["device_sampled"] is False
            path.write_bytes(decoded.stdout + b" ")
            refused = run(*check)
            assert refused.returncode and not refused.stdout and b"SHA-256" in refused.stderr
    for capture in ("ce: 18446744073709551616\nue: 0", "/sys/example", "X" * 129):
        refused = run(*command, capture)
        assert refused.returncode and not refused.stdout
    print("Offline AMDGPU capture: contract, exact counters, unavailable, digest, and invalid input passed")


if __name__ == "__main__":
    main()

import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_amdgpu_ras_evidence import verify


def report(status="decoded", **fields):
    return json.dumps({"schema_version": 1, "kind": "amdgpu_ras_capture", "source": "provided_text",
                       "capture_identity": "unverified", "live_query": "not_performed",
                       "status": status, **fields}).encode()


class AmdgpuEvidence(unittest.TestCase):
    def test_preserves_exact_counters_without_attribution(self):
        raw = report(corrected_errors=str(2**64 - 1), uncorrected_errors="0")
        out = verify(raw, hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["counters"], {"corrected_errors": str(2**64 - 1), "uncorrected_errors": "0"})
        self.assertEqual(out["digest_selection"], "matched")
        self.assertEqual(out["device_attribution"], "not_present")
        self.assertFalse(out["device_sampled"])
        self.assertEqual(out["continuity"], "not_assessed")
        with self.assertRaisesRegex(ValueError, "SHA-256"):
            verify(raw + b" ", hashlib.sha256(raw).hexdigest())

    def test_refuses_invented_values_and_scope(self):
        self.assertEqual(verify(report("unavailable"))["counters"], {})
        for raw in (report("unavailable", corrected_errors="0"),
                    report("unavailable", bdf="0000:65:00.0"),
                    report("unavailable", live_query="observed"),
                    report(corrected_errors="00", uncorrected_errors="0"),
                    report(corrected_errors=str(2**64), uncorrected_errors="0"),
                    report(corrected_errors=0, uncorrected_errors="0")):
            with self.assertRaises(ValueError):
                verify(raw)

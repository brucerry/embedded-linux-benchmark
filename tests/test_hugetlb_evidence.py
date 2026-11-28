import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_hugetlb_evidence import COUNTERS, verify


class HugeTLBEvidenceTest(unittest.TestCase):
    def observed(self):
        report = dict(schema_version=1, kind="hugetlb_pool", status="observed", size_kib=2048)
        report["counters"] = {key: {"status": "unavailable"} for key in COUNTERS}
        report["counters"]["free_hugepages"] = dict(status="observed", value="0")
        return report

    def test_zero_missing_and_digest(self):
        raw = json.dumps(self.observed()).encode()
        result = verify(raw, 2048, hashlib.sha256(raw).hexdigest())
        self.assertEqual(result["counters"]["free_hugepages"]["value"], "0")
        self.assertEqual(result["counters"]["nr_hugepages"], {"status": "unavailable"})
        self.assertEqual(result["digest_selection"], "matched")
        self.assertEqual(result["allocation"], "not_assessed")
        for size, digest in ((1024, None), (2048, "0" * 64), (True, None)):
            with self.assertRaises(ValueError):
                verify(raw, size, digest)

    def test_preserves_independent_total_free_reserved_and_surplus(self):
        report = self.observed()
        values = dict(zip(COUNTERS, ("8", "5", "2", "3")))
        report["counters"] = {key: dict(status="observed", value=value)
                              for key, value in values.items()}
        outcome = verify(json.dumps(report).encode(), 2048)
        self.assertEqual(outcome["counters"], report["counters"])
        self.assertEqual(outcome["counter_unit"], "pages")
        self.assertNotIn("persistent_pages", outcome)
        self.assertNotIn("available_pages", outcome)
        # Independently read values may not describe one atomic pool state.
        report["counters"]["nr_hugepages"]["value"] = "1"
        outcome = verify(json.dumps(report).encode(), 2048)
        self.assertEqual(outcome["counters"], report["counters"])

    def test_withheld_pool_cannot_supply_counters(self):
        report = self.observed()
        for state in ("unavailable", "inaccessible", "invalid", "changed"):
            report["status"] = state
            with self.assertRaises(ValueError):
                verify(json.dumps(report).encode(), 2048)
            omitted = {k: v for k, v in report.items() if k != "counters"}
            self.assertEqual(verify(json.dumps(omitted).encode(), 2048)["status"], state)

    def test_reject_bad_counts_and_duplicate_keys(self):
        report = self.observed()
        for value in (0, True, "00", "-1", "18446744073709551616", "1.5"):
            report["counters"]["free_hugepages"]["value"] = value
            with self.assertRaises(ValueError):
                verify(json.dumps(report).encode(), 2048)
        with self.assertRaises(ValueError):
            verify(b'{"status":"observed","status":"unavailable"}', 2048)

    def test_offline_handoff_cli_requires_selected_size_and_digest(self):
        raw = json.dumps(self.observed()).encode()
        script = Path(__file__).resolve().parents[1] / "scripts/check_hugetlb_evidence.py"
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / "hugetlb.json"
            report.write_bytes(raw)
            digest = hashlib.sha256(raw).hexdigest()
            command = [sys.executable, "-B", str(script), "--report", str(report)]
            allowed = subprocess.run(
                command + ["--size-kib", "2048", "--expected-sha256", digest],
                cwd=tmp, capture_output=True, check=True, text=True,
            )
            receipt = json.loads(allowed.stdout)
            self.assertEqual(receipt["report_sha256"], digest)
            self.assertEqual(receipt["hardware_identity"], "not_verified")
            self.assertFalse(receipt["device_sampled"])
            for size, selected in (("1024", digest), ("2048", "0" * 64)):
                refused = subprocess.run(
                    command + ["--size-kib", size, "--expected-sha256", selected],
                    cwd=tmp, capture_output=True, check=False, text=True,
                )
                self.assertEqual(refused.returncode, 1)
                self.assertEqual(refused.stdout, "")

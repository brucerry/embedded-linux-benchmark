import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_cpu_throttle_evidence import verify


def report(**overrides):
    fields = dict(schema_version=1, kind="cpu_thermal_throttle", cpu=0, status="observed",
                  core_throttle_count={"status": "observed", "value": "0"},
                  package_throttle_count={"status": "unavailable"})
    return json.dumps({**fields, **overrides}).encode()


class CPUThrottleEvidence(unittest.TestCase):
    def test_preserves_scopes_and_zero_without_aggregation(self):
        raw = report()
        out = verify(raw, 0, hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["observations"]["core_throttle_count"]["value"], "0")
        self.assertEqual(out["observations"]["package_throttle_count"]["status"], "unavailable")
        self.assertEqual(out["package_aggregation"], "not_supported")
        self.assertEqual(out["boot_continuity"], "not_verified")
        self.assertEqual(out["current_throttling"], "not_assessed")
        self.assertFalse(out["device_sampled"])

    def test_requires_exact_cpu_digest_and_unsigned_counts(self):
        for value in (True, 0, "-1", "01", "18446744073709551616"):
            with self.assertRaises(ValueError):
                verify(report(core_throttle_count={"status": "observed", "value": value}), 0)
        for raw, cpu, digest in ((report(cpu=True), 0, None), (report(), 1, None),
                                 (report(status="changed"), 0, None),
                                 (report(), 0, "1" * 64)):
            with self.assertRaises(ValueError):
                verify(raw, cpu, digest)

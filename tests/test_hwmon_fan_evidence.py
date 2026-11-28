import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_hwmon_fan_evidence import verify


def capture(**fields):
    return json.dumps({"schema_version": 1, "kind": "hwmon_fan", "sensor": "hwmon4",
                       "channel": 1, "status": "observed", **fields}).encode()


class HwmonFanEvidence(unittest.TestCase):
    def test_distinguishes_zero_from_missing_without_cooling_verdict(self):
        for value in ("0", "1250", "18446744073709551615"):
            raw = capture(raw_value=value, unit="rpm")
            result = verify(raw, "hwmon4", 1, hashlib.sha256(raw).hexdigest())
            self.assertEqual(result["rpm"], value)
            self.assertEqual(result["digest_selection"], "matched")
            self.assertFalse(result["device_sampled"])
            self.assertEqual(result["cooling_health"], "not_assessed")
        self.assertIsNone(verify(capture(status="unavailable"), "hwmon4", 1)["rpm"])

    def test_rejects_wrong_contract_selection_units_and_counts(self):
        good = capture(raw_value="0", unit="rpm")
        examples = [capture(raw_value=value, unit="rpm") for value in
                    (0, True, "00", "-1", "18446744073709551616")]
        examples.extend([
            capture(raw_value="0", unit="unknown"),
            capture(raw_value="0", unit="rpm", channel=True),
            capture(raw_value="0", unit="rpm", kind="hwmon_temperature"),
            capture(raw_value="0", unit="rpm", status="changed"),
            b'{"status":"observed","status":"changed"}',
        ])
        for raw in examples:
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                verify(raw, "hwmon4", 1)
        for sensor, channel, digest in (("hwmon5", 1, None), ("hwmon4", 2, None),
                                         ("hwmon4", 1, "1" * 64)):
            with self.assertRaises(ValueError):
                verify(good, sensor, channel, digest)

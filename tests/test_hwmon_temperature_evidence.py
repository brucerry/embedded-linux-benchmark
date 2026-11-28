import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_hwmon_temperature_evidence import verify


def capture(**fields):
    return json.dumps({"schema_version": 2, "kind": "hwmon_temperature", "sensor": "hwmon4",
                       "channel": 1, "status": "observed", **fields}).encode()


class HwmonEvidence(unittest.TestCase):
    def test_signed_and_zero_values_and_unavailable(self):
        for value in ("-250", "0", "300000"):
            raw = capture(raw_value=value, unit="unknown")
            d = verify(raw, "hwmon4", 1, hashlib.sha256(raw).hexdigest())
            self.assertEqual(d["raw_value"], value)
            self.assertEqual(d["digest_selection"], "matched")
            self.assertFalse(d["device_sampled"])
        self.assertIsNone(verify(capture(status="unavailable"), "hwmon4", 1)["raw_value"])

    def test_refuses_cross_channel_and_malformed_reports(self):
        good = capture(raw_value="0", unit="unknown")
        for raw, sensor, channel, sha in (
            (good, "hwmon4", 2, None),
            (good, "hwmon5", 1, None),
            (good + b"\n", "hwmon4", 1, hashlib.sha256(good).hexdigest()),
            (capture(status="changed", raw_value="0", unit="unknown"), "hwmon4", 1, None),
            (capture(raw_value="9223372036854775808", unit="unknown"), "hwmon4", 1, None),
            (capture(raw_value="-0", unit="unknown"), "hwmon4", 1, None),
            (capture(raw_value="0", unit="unknown", channel=True), "hwmon4", 1, None),
        ):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                verify(raw, sensor, channel, sha)


    def test_in_range_voltage_and_legacy_labels_are_not_temperatures(self):
        for raw in (capture(raw_value="950", unit="unknown"),
                    capture(schema_version=1, temperature_millicelsius="950")):
            result = verify(raw, "hwmon4", 1)
            self.assertEqual(result["raw_value"], "950")
            self.assertEqual(result["unit"], "unknown")
            self.assertNotIn("temperature_millicelsius", result)
        with self.assertRaises(ValueError):
            verify(capture(raw_value="950", unit="millicelsius"), "hwmon4", 1)

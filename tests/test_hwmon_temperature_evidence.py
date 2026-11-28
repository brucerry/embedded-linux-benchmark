import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_hwmon_temperature_evidence import verify


def capture(**fields):
    return json.dumps({"schema_version": 1, "kind": "hwmon_temperature", "sensor": "hwmon4",
                       "channel": 1, "status": "observed", **fields}).encode()


class HwmonEvidence(unittest.TestCase):
    def test_signed_and_zero_values_and_unavailable(self):
        for value in ("-250", "0", "300000"):
            raw = capture(temperature_millicelsius=value)
            d = verify(raw, "hwmon4", 1, hashlib.sha256(raw).hexdigest())
            self.assertEqual(d["temperature_millicelsius"], value)
            self.assertEqual(d["digest_selection"], "matched")
            self.assertFalse(d["device_sampled"])
        self.assertIsNone(verify(capture(status="unavailable"), "hwmon4", 1)["temperature_millicelsius"])

    def test_refuses_cross_channel_and_malformed_reports(self):
        good = capture(temperature_millicelsius="0")
        for raw, sensor, channel, sha in (
            (good, "hwmon4", 2, None),
            (good, "hwmon5", 1, None),
            (good + b"\n", "hwmon4", 1, hashlib.sha256(good).hexdigest()),
            (capture(status="changed", temperature_millicelsius="0"), "hwmon4", 1, None),
            (capture(temperature_millicelsius="300001"), "hwmon4", 1, None),
            (capture(temperature_millicelsius="-0"), "hwmon4", 1, None),
            (capture(temperature_millicelsius="0", channel=True), "hwmon4", 1, None),
        ):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                verify(raw, sensor, channel, sha)

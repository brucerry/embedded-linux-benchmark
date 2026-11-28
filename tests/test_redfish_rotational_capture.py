import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_redfish_rotational_capture import decode


def sample(**overrides):
    return json.dumps({"@odata.type": "#Sensor.v1_11_0.Sensor", "Id": "fan1",
                       "ReadingType": "Rotational", "ReadingUnits": "{rev}/min",
                       "Reading": 0, **overrides}).encode()


class RedfishRotational(unittest.TestCase):
    def test_accepts_only_documented_rpm_spellings(self):
        for raw, value in ((sample(), "0"), (sample(ReadingUnits="RPM", Reading=1250.5), "1250.5")):
            result = decode(raw, "fan1", hashlib.sha256(raw).hexdigest())
            self.assertEqual(result["reading"], {"status": "reported", "value_rpm": value})
            self.assertEqual(result["cooling_health"], "not_assessed")
            self.assertFalse(result["actions_performed"])

    def test_percent_velocity_and_missing_are_not_rpm(self):
        for raw, status in (
            (sample(ReadingType="Percent", ReadingUnits="%", Reading=80), "unsupported_type_or_unit"),
            (sample(ReadingType="RotationalVelocity", ReadingUnits="rad/s"), "unsupported_type_or_unit"),
            (sample(ReadingUnits="rpm"), "unsupported_type_or_unit"),
            (sample(Reading=-1), "invalid"), (sample(Reading=True), "invalid"),
            (sample(Reading=None), "unavailable"),
        ):
            self.assertEqual(decode(raw, "fan1")["reading"], {"status": status})

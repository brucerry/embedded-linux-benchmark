import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_redfish_temperature_capture import decode


def sample(**overrides):
    return json.dumps({"@odata.type": "#Sensor.v1_11_0.Sensor", "Id": "inlet",
                       "ReadingType": "Temperature", "ReadingUnits": "Cel", "Reading": 0,
                       "Oem": {"secret": "not exported"}, **overrides}).encode()


class RedfishTemperature(unittest.TestCase):
    def test_preserves_zero_and_original_decimal(self):
        for raw, expected in ((sample(), "0"), (sample(Reading=-7.125), "-7.125")):
            result = decode(raw, "inlet", hashlib.sha256(raw).hexdigest())
            self.assertEqual(result["reading"], {"status": "reported", "value_celsius": expected})
            self.assertEqual(result["live_request"], "not_performed")
            self.assertNotIn("secret", json.dumps(result))

    def test_distinguishes_missing_type_units_and_invalid_values(self):
        for raw, state in ((sample(Reading=None), "unavailable"),
                           (sample(ReadingUnits="mV"), "unsupported_type_or_unit"),
                           (sample(Reading=True), "invalid"),
                           (sample(Reading=1e40), "invalid")):
            self.assertEqual(decode(raw, "inlet")["reading"]["status"], state)

    def test_refuses_wrong_id_schema_and_digest(self):
        for raw, name, digest in ((sample(), "outlet", None),
                                  (sample(), "inlet", "0" * 64),
                                  (sample(**{"@odata.type": "#Thermal.v1_0_0.Thermal"}), "inlet", None)):
            with self.assertRaises(ValueError):
                decode(raw, name, digest)

import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_thermal_cooling_evidence import verify

DEVICE = "cooling_device0"
BASE = dict(schema_version=1, kind="thermal_cooling_state", device=DEVICE, status="observed")


def encode(value):
    return json.dumps(value).encode()


def report():
    return {**BASE, "type": {"status": "observed", "value": "Processor"},
            "cur_state": {"status": "observed", "value": "0"},
            "max_state": {"status": "unavailable"}}


class ThermalCoolingEvidence(unittest.TestCase):
    def test_zero_and_missing_are_distinct(self):
        raw = encode(report())
        result = verify(raw, DEVICE, hashlib.sha256(raw).hexdigest())
        self.assertEqual(result["observations"]["cur_state"]["value"], "0")
        self.assertEqual(result["observations"]["max_state"], {"status": "unavailable"})
        self.assertEqual(result["throttling"], "not_assessed")
        self.assertFalse(result["device_sampled"])
        self.assertEqual(verify(encode({**BASE, "status": "changed"}), DEVICE)["observations"], {})

    def test_wrong_selection_digest_shape_and_values_refused(self):
        for raw, name, digest in (
            (encode(report()), "cooling_device1", None),
            (encode(report()), DEVICE, "1" * 64),
            (encode({**report(), "status": "changed"}), DEVICE, None),
            (encode({**report(), "cur_state": {"status": "observed", "value": 0}}), DEVICE, None),
            (encode({**report(), "max_state": {"status": "observed", "value": "-1"}}), DEVICE, None),
            (encode({**report(), "type": {"status": "observed", "value": "bad\nname"}}), DEVICE, None),
        ):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                verify(raw, name, digest)

    def test_independent_samples_need_not_form_an_atomic_pair(self):
        value = report()
        value["cur_state"]["value"] = "8"
        value["max_state"] = {"status": "observed", "value": "2"}
        outcome = verify(encode(value), DEVICE)
        self.assertEqual(outcome["observations"]["cur_state"]["value"], "8")
        self.assertEqual(outcome["observations"]["max_state"]["value"], "2")
        self.assertEqual(outcome["cooling_health"], "not_assessed")

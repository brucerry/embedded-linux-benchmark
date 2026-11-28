import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_tegrastats_evidence import verify

RAIL = "VDD_GPU_SOC"
UNAVAILABLE = {"status": "unavailable"}


def observed(value):
    return {"status": "observed", "value": value}


def capture(**fields):
    q = {"schema_version": 1, "kind": "jetson_tegrastats_capture",
         "live_query": "not_performed", "capture_identity": "unverified",
         "gpu_utilization_percent": observed(0), "gpu_frequency_mhz": observed([306, 0, 1020]),
         "gpu_temperature_millicelsius": observed(-1025),
         "selected_rail": {"label": RAIL, "observation": observed(
             {"instant_mw": 7, "average_mw": 8})}}
    return json.dumps({**q, **fields}).encode()


class JetsonEvidenceTest(unittest.TestCase):
    def test_preserves_zero_and_rail_scope(self):
        raw = capture()
        result = verify(raw, RAIL, hashlib.sha256(raw).hexdigest())
        self.assertEqual(result["selected_rail"]["label"], RAIL)
        self.assertEqual(result["metrics"]["gpu_frequency_mhz"]["value"], [306, 0, 1020])
        self.assertEqual(result["gpu_power_scope"], "not_inferred")
        self.assertFalse(result["device_sampled"])

    def test_refuses_wrong_rail_digest_units_and_values(self):
        for raw, rail, digest in (
            (capture(), None, None),
            (capture(), "VDD_GPU", None),
            (capture(), RAIL, "0" * 64),
            (capture(gpu_utilization_percent=observed(True)), RAIL, None),
            (capture(gpu_frequency_mhz=observed([0] * 9)), RAIL, None),
            (capture(gpu_temperature_millicelsius=observed(1000000)), RAIL, None),
            (capture(live_query="performed"), RAIL, None),
        ):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                verify(raw, rail, digest)
        self.assertIsNone(verify(capture(selected_rail=None))["selected_rail"])
        raw = capture(gpu_utilization_percent=UNAVAILABLE, gpu_frequency_mhz=UNAVAILABLE,
                      gpu_temperature_millicelsius=UNAVAILABLE, selected_rail=None)
        with self.assertRaises(ValueError):
            verify(raw)

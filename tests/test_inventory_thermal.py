import json
import unittest

from test_inventory_evidence_compare import capture
from check_inventory_thermal import evaluate


def window(delta="-1500", wait=3000):
    q = json.loads(capture(elapsed=60001))
    def value(text):
        return {"status": "observed", "value": text}
    def zone(idx, temp):
        return {"zone": f"thermal_zone{idx}", "type": value("fixture"),
                "temperature_millicelsius": value(temp)}
    before = {"status": "observed", "zones": [zone(0, "1500"), zone(1, "0")]}
    after = {"status": "observed", "zones": [zone(0, "0")]}
    q["inventory"].update(thermal=before, thermal_window={
        "requested_wait_ms": wait, "sample_start_interval_ms": wait,
        "before": before, "after": after,
        "differences": [
            {"zone": "thermal_zone0", "difference_millicelsius": {
                "status": "observed_difference", "value": delta}},
            {"zone": "thermal_zone1", "difference_millicelsius": {"status": "not_comparable"}},
        ]})
    return json.dumps(q).encode()


class InventoryThermal(unittest.TestCase):
    def test_selected_window_distinguishes_missing_and_observed_zero(self):
        result = evaluate(window(), "thermal_zone0")
        self.assertEqual(result["status"], "compared")
        self.assertEqual(result["after"]["temperature_millicelsius"]["value"], "0")
        self.assertEqual(result["difference_millicelsius"]["value"], "-1500")
        self.assertEqual(result["workload_execution"], "none")
        self.assertEqual(evaluate(window(), "thermal_zone1")["status"], "not_comparable")
        self.assertEqual(evaluate(window(), "thermal_zone2")["status"], "not_reported")

    def test_rejects_altered_report_or_noncanonical_zone(self):
        for raw, zone, digest in (
            (window("-1499"), "thermal_zone0", None),
            (window(), "thermal_zone00", None),
            (window(), "thermal_zone0", "0"*64),
            (window(wait=60001), "thermal_zone0", None),
            (capture(), "thermal_zone0", None),
        ):
            with self.subTest(zone=zone, digest=digest), self.assertRaises(ValueError):
                evaluate(raw, zone, digest)

import copy
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_network_health_window import KEYS, verify


def dump(obj):
    return json.dumps(obj).encode()


def observed(v):
    return {"status": "observed", "value": v}


def fixture():
    snapshot = {"schema_version": 1, "interface": "eth0", "status": "observed",
                "carrier": observed(True), "speed_mbps": observed("1000"),
                "duplex": observed("full"), "operstate": observed("up"),
                "counters": {k: {"status": "unavailable"} for k in KEYS}}
    a, b = copy.deepcopy(snapshot), copy.deepcopy(snapshot)
    a["counters"]["rx_bytes"] = observed("18446744073709551614")
    b["counters"]["rx_bytes"] = observed("18446744073709551615")
    delta = {"status": "observed_differences", "comparable_counters": 1, "counter_coverage": "partial",
             "counters": {k: observed("1") if k == "rx_bytes"
                           else {"status": "endpoint_unavailable"} for k in KEYS}}
    return {"schema_version": 1, "mode": "two_passive_snapshots", "requested_wait_ms": 100,
            "sample_start_interval_ms": 101, "continuity": "same_identity_observed",
            "before": a, "after": b, "delta": delta}


class NetworkHealthWindowTest(unittest.TestCase):
    def test_exact_u64_and_unavailable(self):
        result = verify(dump(fixture()), "eth0", 100)
        self.assertEqual(result["delta"]["counters"]["rx_bytes"], observed("1"))
        self.assertEqual(result["identity_attestation"], "not_verified")

    def test_bad_delta_count_interface_and_interval_refused(self):
        for key, bad in [("comparable_counters", True), ("counter_coverage", "complete")]:
            item = fixture()
            item["delta"][key] = bad
            with self.assertRaisesRegex(ValueError, "delta disagrees"):
                verify(dump(item), "eth0", 100)
        with self.assertRaisesRegex(ValueError, "selected interface"):
            verify(dump(fixture()), "eth1", 100)
        with self.assertRaisesRegex(ValueError, "sample interval"):
            verify(dump(fixture()), "eth0", 200)

    def test_reset_and_changed_identity_withhold(self):
        item = fixture()
        item["after"]["counters"]["rx_bytes"] = observed("0")
        with self.assertRaisesRegex(ValueError, "delta disagrees"):
            verify(dump(item), "eth0", 100)
        item["delta"] = {"status": "counter_decreased_or_reset"}
        self.assertEqual(verify(dump(item), "eth0", 100)["delta"], item["delta"])
        item.update(continuity="identity_changed", delta={"status": "not_comparable"})
        self.assertEqual(verify(dump(item), "eth0", 100)["delta"], item["delta"])

    def test_absence_does_not_turn_into_zero(self):
        item = fixture()
        for side in ("before", "after"):
            item[side]["counters"]["rx_bytes"] = {"status": "unavailable"}
        item["delta"].update(counter_coverage="none", comparable_counters=0)
        item["delta"]["counters"]["rx_bytes"] = {"status": "endpoint_unavailable"}
        self.assertEqual(verify(dump(item), "eth0", 100)["delta"]["counter_coverage"], "none")

    def test_withheld_sample_cannot_assert_continuity(self):
        item = fixture()
        item["after"] = {"schema_version": 1, "interface": "eth0", "status": "changed"}
        with self.assertRaisesRegex(ValueError, "withheld samples"):
            verify(dump(item), "eth0", 100)
        item.update(continuity="unknown", delta={"status": "not_comparable"})
        self.assertEqual(verify(dump(item), "eth0", 100)["delta"], item["delta"])

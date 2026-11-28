import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_passive_rdma_report import SCOPE, verify


def pack(fields=None, **overrides):
    values = {"state": "4: ACTIVE", "phys_state": "5: LinkUp",
              "link_layer": "Ethernet", "rate": "2.5 Gb/sec (1X SDR)"}
    return json.dumps({**SCOPE, "selection": {"device": "rxe0", "port": 1},
                       "status": "observed",
                       "attributes": fields if fields is not None else
                       {k: {"status": "reported", "raw": v} for k, v in values.items()},
                       **overrides}).encode()


class PassiveRdmaReport(unittest.TestCase):
    def test_exact_golden_fixture_shared_with_rust_producer(self):
        fixture = Path(__file__).with_name("fixtures") / "rdma-passive-rxe-report.json"
        raw = fixture.read_bytes()
        outcome = verify(raw, "rxe0", 1, hashlib.sha256(raw).hexdigest())
        self.assertEqual(outcome["attributes"]["link_layer"]["raw"], "Ethernet")
        self.assertEqual(outcome["attributes"]["rate"]["raw"], "2.5 Gb/sec (1X SDR)")
        self.assertFalse(outcome["throughput_measured"])

    def test_saved_bytes_and_selection_remain_distinct_from_physical_identity(self):
        raw = pack()
        result = verify(raw, "rxe0", 1, hashlib.sha256(raw).hexdigest())
        self.assertEqual(result["digest_selection"], "matched")
        self.assertEqual(result["hardware_authenticity"], "not_verified")
        self.assertEqual(result["ethernet_line_rate"], "not_assessed")
        self.assertEqual(result["attributes"]["rate"]["raw"], "2.5 Gb/sec (1X SDR)")
        self.assertEqual(verify(raw, "rxe0", 1)["digest_selection"], "unbound")
        with self.assertRaisesRegex(ValueError, "differs from expected SHA-256"):
            verify(raw + b" ", "rxe0", 1, hashlib.sha256(raw).hexdigest())

    def test_unavailable_and_nondefault_selections_are_checked(self):
        self.assertEqual(verify(pack({}, status="unavailable"), "rxe0", 1)["attributes"], {})
        for raw in (pack(status="unavailable"), pack({"rate": {"status": "unavailable"}}),
                    pack(traffic_started=True), pack(throughput_measured=0),
                    pack(selection={"device": "rxe0", "port": True})):
            with self.assertRaises(ValueError):
                verify(raw, "rxe0", 1)
        with self.assertRaisesRegex(ValueError, "expected selection"):
            verify(pack(), "rxe0", 2)

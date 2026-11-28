import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_rdma_port_capture import decode

FIXTURE = {"state": "4: ACTIVE\n", "phys_state": "5: LinkUp\n",
           "link_layer": "InfiniBand\n", "rate": "400 Gb/sec (4X NDR)\n"}


class RdmaPortCapture(unittest.TestCase):
    def test_kernel_shaped_fixture_is_not_throughput_or_hardware_proof(self):
        raw = json.dumps(FIXTURE).encode()
        out = decode(raw)
        self.assertEqual(out["capture_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["attributes"]["rate"]["gigabits_per_second"], "400")
        self.assertEqual(out["attributes"]["state"]["code"], "4")
        self.assertEqual(out["port_attribution"], "not_present")
        self.assertFalse(out["throughput_measured"])
        self.assertFalse(out["device_sampled"])

    def test_pins_exact_saved_bytes_not_hardware(self):
        raw = json.dumps(FIXTURE).encode()
        digest = hashlib.sha256(raw).hexdigest()
        self.assertEqual(decode(raw)["digest_selection"], "unbound")
        matched = decode(raw, digest)
        self.assertEqual(matched["digest_selection"], "matched")
        self.assertEqual(matched["hardware_authenticity"], "not_verified")
        with self.assertRaisesRegex(ValueError, "differs from expected SHA-256"):
            decode(raw + b" ", digest)
        with self.assertRaisesRegex(ValueError, "lowercase SHA-256"):
            decode(raw, digest.upper())

    def test_whole_half_zero_and_absent_are_distinct(self):
        out = decode(json.dumps({**FIXTURE, "link_layer": None, "state": "0: NOP",
                                 "rate": "2.5 Gb/sec (1X SDR)"}).encode())["attributes"]
        self.assertEqual(out["rate"]["gigabits_per_second"], "2.5")
        self.assertEqual(out["state"]["code"], "0")
        self.assertEqual(out["link_layer"], {"status": "unavailable"})

    def test_refuses_out_of_contract_values(self):
        for bad in ({"rate": "400GbE"}, {"phys_state": "999: bad"},
                    {"state": "4: ACTIVE\n0: NOP"}, {"rate": "9.9 Gb/sec (1X SDR)"},
                    {"link_layer": True}, {"rate": "x" * 65}, {"port": 1}):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                decode(json.dumps({**FIXTURE, **bad}).encode())
        with self.assertRaises(ValueError):
            decode(b"{" + b" "*4096 + b"}")

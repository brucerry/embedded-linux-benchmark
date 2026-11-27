import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_peer_route_evidence import correlate, inspect  # noqa: E402


def row(source, peer, dev, **extra):
    return json.dumps([{"dst": peer, "from": source, "dev": dev, "flags": [], **extra}]).encode()


class PeerRouteTest(unittest.TestCase):
    def test_direct_paths_are_route_evidence_only(self):
        a, b = "10.91.12.14", "10.91.12.15"
        result = correlate(row(a, b, "veth1"), row(b, a, "veth2"), "veth1", a, "veth2", b)
        self.assertEqual(result["status"], "matched_route_lookups")
        self.assertEqual(result["actual_socket_path"], "not_verified")
        self.assertEqual(result["physical_path"], "not_verified")

    def test_local_route_does_not_pass_when_interface_matches(self):
        a, b = "10.91.12.14", "10.91.12.15"
        result = correlate(row(a, b, "eth7", type="local"), row(b, a, "eth8", cache=["local"]),
                           "eth7", a, "eth8", b)
        self.assertEqual(result["status"], "host_local_observed")
        self.assertEqual(result["sender"]["observed_interface"], "eth7")
        self.assertEqual(result["physical_path"], "not_verified")

    def test_gateway_wrong_interface_and_wrong_source(self):
        a, b = "192.168.99.5", "192.168.99.6"
        self.assertEqual(inspect(row(a, b, "eth7", gateway="192.168.99.1"), "eth7", a, b)["status"],
                         "routed_or_ambiguous")
        self.assertEqual(inspect(row(a, b, "eth7"), "eth8", a, b)["status"], "interface_mismatch")
        for data in (row(b, a, "eth7"), b"[]", b'[{"dev":"eth7","dev":"eth8"}]'):
            with self.assertRaises(ValueError):
                inspect(data, "eth7", a, b)

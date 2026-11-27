import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from plan_network_peer import plan  # noqa: E402

ARGS = ["eth1", "172.31.95.1/30", "enP1p1s0", "172.31.95.2/30", 5555,
        "0123456789abcdef0123456789abcdef", 1, 20, 5000]


class PeerPlanTest(unittest.TestCase):
    def test_source_binds_are_mirrored_with_identical_budgets(self):
        out = plan(*ARGS)
        send, receive = out["sender"], out["receiver"]
        self.assertEqual(send["peer_argv"][2:7],
                         ["peer-client", "--bind", "172.31.95.1", "--peer", "172.31.95.2"])
        self.assertEqual(receive["peer_argv"][2:7],
                         ["peer-server", "--bind", "172.31.95.2", "--peer", "172.31.95.1"])
        self.assertEqual(send["peer_argv"][7:], receive["peer_argv"][7:])
        self.assertEqual(receive["health_argv"][-1], "enP1p1s0")
        self.assertEqual(out["expected_payload_bytes"], 1048576)
        self.assertEqual(out["execution"], "none")
        self.assertFalse(out["path_verified"])

    def test_different_interface_and_point_to_point_prefix_without_speed_requirement(self):
        args = ["lan3", "10.91.12.14/31", "enx0123456789ab", "10.91.12.15/31", 7777,
                ARGS[5], 2, 7, 9000]
        out = plan(*args)
        self.assertEqual(out["receiver"]["address"], "10.91.12.15")
        self.assertEqual(out["sender"]["health_argv"][-1], "lan3")
        self.assertEqual(out["expected_payload_bytes"], 2 * 1024**2)
        self.assertEqual(out["expected_round_trips"], 7)
        self.assertNotIn("speed", out)
        self.assertNotIn("speed", out["receiver"])
        self.assertFalse(out["physical_measurement_verified"])

    def test_refuses_unsafe_or_mismatched_endpoints_and_budgets(self):
        for i, value in ((0, "lo"), (0, "eth0;id"), (1, "8.8.8.8/24"),
                         (1, "127.0.0.1/30"), (1, "172.31.95.3/30"),
                         (3, "172.31.95.1/30"), (3, "172.31.96.2/30"),
                         (3, "172.31.95.2/32"), (4, 0), (5, "0" * 32),
                         (6, 65), (7, 1001), (8, 30001)):
            args = ARGS[:]
            args[i] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                plan(*args)

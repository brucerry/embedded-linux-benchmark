import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_pcie_locality_evidence import verify

BDF = "0000:01:00.0"


def capture(**fields):
    return json.dumps({"schema_version": 1, "kind": "pcie_locality", "device": BDF,
                       "status": "observed", "numa_node": {"status": "observed", "value": 0},
                       "local_cpu_ids": {"status": "observed", "value": [0, 1, 4]}, **fields}).encode()


class PcieLocality(unittest.TestCase):
    def test_observed_zero_unknown_and_cpu_list_states(self):
        raw = capture()
        out = verify(raw, BDF, hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["fields"]["numa_node"]["value"], 0)
        self.assertEqual(out["fields"]["local_cpu_ids"]["value"], [0, 1, 4])
        self.assertEqual(out["affinity"], "not_assessed")
        unknown = verify(capture(numa_node={"status": "unknown"}), BDF)
        self.assertEqual(unknown["fields"]["numa_node"], {"status": "unknown"})
        missing = {"schema_version": 1, "kind": "pcie_locality", "device": BDF, "status": "unavailable"}
        self.assertEqual(verify(json.dumps(missing).encode(), BDF)["fields"], {})

    def test_refuses_digest_device_and_contradictory_representations(self):
        for raw, device, digest in (
            (capture(), "0000:01:00.1", None),
            (capture(), BDF, "0"*64),
            (capture(numa_node={"status": "observed", "value": -1}), BDF, None),
            (capture(numa_node={"status": "unknown", "value": 0}), BDF, None),
            (capture(local_cpu_ids={"status": "observed", "value": [0, 0]}), BDF, None),
            (capture(local_cpu_ids={"status": "observed", "value": [False]}), BDF, None),
            (capture(status="changed"), BDF, None),
        ):
            with self.subTest(raw=raw, device=device), self.assertRaises(ValueError):
                verify(raw, device, digest)

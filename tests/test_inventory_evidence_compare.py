import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from compare_inventory_evidence import FIELDS, compare


def capture(*, architecture="aarch64", kernel="6.6.1", firmware=None, elapsed=5):
    x = {"schema_version": 1, "process_architecture": architecture,
         **{name: {"status": "unavailable"} for name in FIELDS}}
    x["kernel_release"] = {"status": "observed", "value": kernel}
    x["dmi_firmware_version"] = firmware or {"status": "unavailable"}
    return json.dumps({"evidence_schema_version": 1, "kind": "passive_inventory",
                       "started_unix_ms": 1234, "finished_unix_ms": 1230,
                       "elapsed_monotonic_ms": elapsed, "inventory": x}).encode()


class InventoryComparison(unittest.TestCase):
    def test_changes_are_not_hardware_identity(self):
        a = capture()
        b = capture(kernel="6.6.2", firmware={"status": "inaccessible"})
        d = compare(a, b, hashlib.sha256(a).hexdigest(), hashlib.sha256(b).hexdigest())
        self.assertEqual(d["fields"]["kernel_release"]["status"], "changed")
        self.assertEqual(d["fields"]["process_architecture"]["status"], "unchanged")
        self.assertEqual(d["fields"]["device_tree_model"]["status"], "not_comparable")
        self.assertEqual(d["fields"]["dmi_firmware_version"]["after"], {"status": "inaccessible"})
        self.assertEqual(d["hardware_identity"], "not_verified")
        self.assertEqual(d["boot_continuity"], "not_assessed")
        self.assertFalse(d["device_sampled"])

    def test_invalid_or_substituted_capture(self):
        good = capture()
        sha = hashlib.sha256(good).hexdigest()
        for a, b, left, right in (
            (good, good, sha, None),
            (good + b"\n", good, sha, sha),
            (capture(architecture="bad arch"), good, None, None),
            (capture(kernel="bad\x00value"), good, None, None),
            (capture(elapsed=True), good, None, None),
            (capture(firmware={"status": "unavailable", "value": "0"}), good, None, None),
        ):
            with self.subTest(a=a, left=left), self.assertRaises(ValueError):
                compare(a, b, left, right)


    def test_build_labels_compare_separately_and_older_captures_remain_compatible(self):
        old = capture()
        obj = json.loads(old)
        obj["inventory"]["distribution_version"] = {"status": "observed", "value": "23.05.6"}
        obj["inventory"]["distribution_build_id"] = {"status": "observed", "value": "r12345-abc123"}
        before = json.dumps(obj).encode()
        obj["inventory"]["distribution_build_id"]["value"] = "r12346-def456"
        after = json.dumps(obj).encode()
        labels = compare(before, after)["fields"]
        self.assertEqual(labels["distribution_build_id"]["status"], "changed")
        self.assertEqual(labels["distribution_version"]["status"], "unchanged")
        self.assertEqual(labels["kernel_release"]["status"], "unchanged")
        self.assertEqual(compare(old, after)["fields"]["distribution_build_id"]["status"], "not_comparable")
        obj["inventory"]["distribution_build_id"] = None
        with self.assertRaises(ValueError):
            compare(old, json.dumps(obj).encode())

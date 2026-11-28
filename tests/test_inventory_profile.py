import hashlib
import json
import unittest

from test_inventory_evidence_compare import capture
from check_inventory_profile import evaluate


def profile(**expected):
    return json.dumps({"schema_version": 1, "expected": expected}).encode()


class InventoryProfile(unittest.TestCase):
    def test_compares_exact_firmware_and_kernel_labels_without_claiming_qualification(self):
        data = capture(firmware={"status": "observed", "value": "v1.2"})
        result = evaluate(data, profile(kernel_release="6.6.1", dmi_firmware_version="v1.2"),
                          hashlib.sha256(data).hexdigest())
        self.assertEqual(result["status"], "match")
        self.assertEqual(result["hardware_identity"], "not_verified")
        self.assertEqual(result["qualification"], "not_assessed")

    def test_missing_and_mismatched_labels_are_separate(self):
        data = capture(firmware={"status": "inaccessible"})
        self.assertEqual(evaluate(data, profile(dmi_firmware_version="v1.2"))["status"], "incomplete")
        answer = evaluate(data, profile(kernel_release="6.6", dmi_firmware_version="v1.2"))
        self.assertEqual(answer["status"], "mismatch")
        self.assertEqual(answer["fields"]["dmi_firmware_version"]["status"], "not_comparable")

    def test_rejects_bad_profile_and_capture_selection(self):
        data = capture()
        for plan in (profile(), profile(serial_number="x"), profile(kernel_release="*\n"),
                     profile(kernel_release=True), profile(kernel_release="x"*257),
                     b'{"schema_version":1,"expected":{"kernel_release":"x","kernel_release":"y"}}'):
            with self.subTest(plan=plan), self.assertRaises(ValueError):
                evaluate(data, plan)
        with self.assertRaises(ValueError):
            evaluate(data, profile(kernel_release="6.6.1"), "1"*64)


    def test_build_label_expectation_is_optional_for_older_captures(self):
        data = capture()
        key = "distribution_build_id"
        expected = profile(distribution_build_id="r12345-abc123")
        self.assertEqual(evaluate(data, expected)["status"], "incomplete")
        obj = json.loads(data)
        obj["inventory"][key] = {"status": "observed", "value": "r12345-abc123"}
        self.assertEqual(evaluate(json.dumps(obj).encode(), expected)["status"], "match")

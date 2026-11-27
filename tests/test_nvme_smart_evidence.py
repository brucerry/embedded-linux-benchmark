import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_nvme_smart_evidence import receipt  # noqa: E402


class NvmeReceiptTest(unittest.TestCase):
    def setUp(self):
        page = bytearray(512)
        page[0:6] = bytes([2, 44, 1, 255, 10, 123])
        page[48:64] = (2**128 - 1).to_bytes(16, "little")
        page[160:176] = (2**64 + 3).to_bytes(16, "little")
        self.page = bytes(page)
        self.sha = hashlib.sha256(page).hexdigest()
        self.report = {
            "schema_version": 1, "source": "provided_smart_log_page", "device_sampled": False,
            "critical_warning_raw": 2, "composite_temperature_kelvin": 300,
            "available_spare_percent": None, "available_spare_threshold_percent": 10,
            "percent_used_raw": 123, "data_units_read": {"status": "unreported", "raw": "0"},
            "data_units_written": {"status": "reported", "raw": "340282366920938463463374607431768211455"},
            "power_on_hours_raw": "0", "unsafe_shutdowns_raw": "0", "media_errors_raw": "18446744073709551619",
            "error_log_entries_raw": "0",
        }

    def test_exact_page_and_report_not_hardware_attestation(self):
        raw = json.dumps(self.report).encode()
        out = receipt(self.page, raw, self.sha)
        self.assertEqual(out["page_sha256"], self.sha)
        self.assertEqual(out["report_sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["capture_provenance"], "not_verified")
        self.assertEqual(out["drive_health"], "not_assessed")

    def test_mutated_page_wrong_size_and_forged_report(self):
        raw = json.dumps(self.report).encode()
        for page in (self.page[:-1], b"\x00" + self.page[1:]):
            with self.assertRaises(ValueError):
                receipt(page, raw, self.sha)
        for key, value in (("device_sampled", 0), ("schema_version", True),
                           ("media_errors_raw", "0"), ("critical_warning_raw", 0)):
            with self.subTest(key=key), self.assertRaises(ValueError):
                receipt(self.page, json.dumps({**self.report, key: value}).encode(), self.sha)

import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from compare_nvme_smart_evidence import compare  # noqa: E402
import test_nvme_smart_evidence as fixture


class NvmeComparison(unittest.TestCase):
    def capture(self, media, written):
        base = fixture.NvmeReceiptTest()
        base.setUp()
        page = bytearray(base.page)
        page[48:64] = written.to_bytes(16, "little")
        page[160:176] = media.to_bytes(16, "little")
        report = dict(base.report, data_units_written={
            "status": "reported" if written else "unreported", "raw": str(written)
        }, media_errors_raw=str(media))
        return bytes(page), json.dumps(report).encode(), hashlib.sha256(page).hexdigest()

    def test_preserves_wide_values_and_missing_endpoints(self):
        before = self.capture(10, 1)
        after = self.capture(2**80 + 10, 2**128 - 1)
        result = compare(before, after)
        found = result["counters"]
        self.assertEqual(found["media_errors_raw"], {"status": "observed", "value": str(2**80)})
        self.assertEqual(found["data_units_written"]["value"], str(2**128 - 2))
        self.assertEqual(found["data_units_read"], {
            "status": "not_comparable", "reason": "unreported_endpoint",
        })
        self.assertEqual(found["unsafe_shutdowns_raw"], {"status": "observed", "value": "0"})
        self.assertEqual(result["capture_identity"], "not_verified")
        self.assertEqual(result["drive_health"], "not_assessed")
        self.assertIs(result["device_sampled"], False)

    def test_unchanged_reported_group_is_distinct_from_unreported(self):
        # Distinct pages because the media-error count changes. Neither
        # data-unit value proves that no underlying write I/O occurred.
        result = compare(self.capture(10, 1), self.capture(11, 1))
        self.assertEqual(result["counters"]["data_units_written"],
                         {"status": "observed", "value": "0"})
        self.assertEqual(result["counters"]["data_units_read"],
                         {"status": "not_comparable", "reason": "unreported_endpoint"})
        self.assertEqual(result["measurement_scope"], "counter_arithmetic_only")

    def test_decreased_and_unreported_are_not_zero(self):
        result = compare(self.capture(10, 5), self.capture(2, 0))
        self.assertEqual(result["counters"]["media_errors_raw"], {"status": "decreased_or_reset"})
        self.assertEqual(result["counters"]["data_units_written"]["status"], "not_comparable")

    def test_refuses_identical_tampered_and_unpinned_captures(self):
        before, after = self.capture(10, 1), self.capture(11, 1)
        with self.assertRaisesRegex(ValueError, "identical pages"):
            compare(before, before)
        with self.assertRaisesRegex(ValueError, "saved page differs"):
            compare(before, (after[0], after[1], before[2]))
        with self.assertRaisesRegex(ValueError, "decoded report does not match"):
            compare(before, (after[0], before[1], after[2]))

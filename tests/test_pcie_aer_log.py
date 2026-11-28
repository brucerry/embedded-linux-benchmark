import hashlib
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_pcie_aer_log import MAX_MATCHES, verify

LINE = b"[   5.001000] pcieport 0000:00:01.0: AER: Multiple Uncorrected (Non-Fatal) error received: 0000:02:00.0\n"


class AerLog(unittest.TestCase):
    def test_separates_observer_and_printed_source_without_root_cause(self):
        raw = b"unrelated not forwarded\n" + LINE
        out = verify(raw, "0000:00:01.0", hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["notices"], [{"line": 2, "observer_bdf": "0000:00:01.0",
                         "source_id_printed": "0000:02:00.0", "severity": "nonfatal",
                         "multiple_flag": True}])
        self.assertEqual(out["root_cause"], "not_assessed")
        self.assertFalse(out["hardware_errors_counted"])

    def test_no_match_is_not_a_health_result(self):
        out = verify(LINE, "0000:02:00.0")
        self.assertEqual(out["status"], "no_matching_notices")
        self.assertFalse(out["notices"])

    def test_limits_and_exact_bytes(self):
        out = verify(LINE * (MAX_MATCHES + 1), "0000:00:01.0")
        self.assertEqual(len(out["notices"]), MAX_MATCHES)
        self.assertEqual(out["omitted_matching_notices"], 1)
        with self.assertRaisesRegex(ValueError, "expected SHA-256"):
            verify(LINE + b" ", "0000:00:01.0", hashlib.sha256(LINE).hexdigest())
        with self.assertRaisesRegex(ValueError, "observer BDF"):
            verify(LINE, "0000:00:21.0")

    def test_refuses_lookalike_and_supports_raw_kernel_notice(self):
        log = (b"prefix pcieport 0000:00:01.0: Corrected error received: 0000:02:00.0\n"
               b"pcieport 0000:00:01.0: Corrected error received: 0000:02:00.0\n")
        self.assertEqual(verify(log, "0000:00:01.0")["notices"][0]["line"], 2)

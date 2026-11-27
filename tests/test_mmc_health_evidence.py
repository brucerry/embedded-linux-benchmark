import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_mmc_health_evidence import verify  # noqa: E402

CARD = "mmc0:000a"


def enc(value):
    return json.dumps(value).encode()


class MmcReport(unittest.TestCase):
    def setUp(self):
        self.row = {
            "schema_version": 1, "card": CARD, "status": "observed", "card_type": "MMC",
            "pre_eol_info_raw": {"status": "observed", "value": 0},
            "life_time_raw": {"status": "observed",
                              "value": {"type_a_raw": 0, "type_b_raw": 255}},
        }

    def test_raw_zero_does_not_report_wear_or_health(self):
        raw = enc(self.row)
        output = verify(raw, CARD, hashlib.sha256(raw).hexdigest())
        self.assertEqual(output["digest_selection"], "matched")
        self.assertEqual(output["raw_field_states"]["pre_eol_info_raw"], "observed")
        self.assertEqual(output["wear_assessment"], "not_assessed")
        self.assertEqual(output["hardware_authenticity"], "not_verified")
        self.assertIs(output["device_sampled"], False)
        self.row["pre_eol_info_raw"] = {"status": "unavailable"}
        self.assertEqual(verify(enc(self.row), CARD)["raw_field_states"]["pre_eol_info_raw"],
                         "unavailable")

    def test_rejects_inferred_metrics_and_wrong_card_or_hash(self):
        variants = (
            {"status": "observed", "value": True},
            {"status": "observed", "value": 256},
            {"status": "unavailable", "value": 0},
        )
        for value in variants:
            invalid = dict(self.row, pre_eol_info_raw=value)
            with self.assertRaises(ValueError):
                verify(enc(invalid), CARD)
        invalid = copy.deepcopy(self.row)
        invalid["life_time_raw"]["value"]["wear_percent"] = 50
        for obj in (invalid, dict(self.row, status="unsupported_card_type")):
            with self.assertRaises(ValueError):
                verify(enc(obj), CARD)
        with self.assertRaisesRegex(ValueError, "selected card mismatch"):
            verify(enc(self.row), "mmc1:000a")
        with self.assertRaisesRegex(ValueError, "differs from expected SHA-256"):
            verify(enc(self.row), CARD, "0" * 64)

    def test_withheld_card_and_invalid_identity(self):
        unavailable = {"schema_version": 1, "card": CARD, "status": "unavailable"}
        result = verify(enc(unavailable), CARD)
        self.assertEqual(result["card_observation"], "unavailable")
        self.assertEqual(result["raw_field_states"], {})
        for wrong in ("../mmc0:000a", "mmc01:000a", "mmc0:000A", "mmcblk0"):
            with self.assertRaisesRegex(ValueError, "expected MMC card"):
                verify(enc(unavailable), wrong)

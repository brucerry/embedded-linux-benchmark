import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from compare_mmc_health_evidence import compare

CARD = "mmc0:0001"


def make(card=CARD, status="observed", eol=0, a=0, b=1):
    obj = {"schema_version": 1, "card": card, "status": status}
    if status == "observed":
        obj.update(card_type="MMC", pre_eol_info_raw={"status": "observed", "value": eol},
                   life_time_raw={"status": "observed", "value": {"type_a_raw": a, "type_b_raw": b}})
    return json.dumps(obj).encode()


class MMCCompare(unittest.TestCase):
    def test_code_change_is_raw_not_wear(self):
        x = compare(make(), make(b=2), CARD)
        self.assertEqual(x["fields"]["pre_eol_info_raw"]["status"], "unchanged")
        self.assertEqual(x["fields"]["pre_eol_info_raw"]["before"]["value"], 0)
        self.assertEqual(x["fields"]["life_time_raw"]["code_changes"], {"type_a_raw": False, "type_b_raw": True})
        self.assertEqual(x["hardware_identity"], "not_verified")
        self.assertEqual(x["wear_assessment"], "not_assessed")
        self.assertFalse(x["device_sampled"])

    def test_missing_data_never_compares_equal(self):
        a = make(status="unavailable")
        self.assertEqual(compare(a, a, CARD)["fields"]["life_time_raw"]["status"], "not_comparable")
        self.assertEqual(compare(make(), make(status="changed"), CARD)["after_card_observation"], "changed")

    def test_rejects_card_digest_and_raw_shape_mismatch(self):
        a, b = make(), make(b=2)
        sha = lambda raw: hashlib.sha256(raw).hexdigest()
        self.assertEqual(compare(a, b, CARD, sha(a), sha(b))["after_sha256"], sha(b))
        for x, y, left, right in (
            (b, a, sha(a), sha(b)),
            (a, b, sha(a), None),
            (a, make("mmc0:0002"), None, None),
            (a, make(eol=True), None, None),
        ):
            with self.subTest(x=x, y=y), self.assertRaises(ValueError):
                compare(x, y, CARD, left, right)

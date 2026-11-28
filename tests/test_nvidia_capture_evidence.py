import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_nvidia_capture_evidence import verify

DEVICE = "0000:01:00.0"


def fixture():
    return {"schema_version": 1, "kind": "nvidia_nvml_capture", "source": "provided_normalized_text",
            "capture_identity": "unverified", "live_query": "not_performed", "bdf": DEVICE,
            "ecc_scope": "volatile_total", "temperature_celsius": {"status": "unavailable"},
            "power_milliwatts": {"status": "observed", "value": str(2**32 - 1)},
            "corrected_ecc": {"status": "observed", "value": str(2**64 - 1)},
            "uncorrected_ecc": {"status": "observed", "value": "0"}}


def pack(q):
    return json.dumps(q).encode()


class NvidiaCaptureEvidence(unittest.TestCase):
    def test_preserves_exact_observations_and_cannot_assert_continuity(self):
        q = fixture()
        raw = pack(q)
        out = verify(raw, DEVICE, hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["digest_selection"], "matched")
        self.assertEqual(out["metrics"]["corrected_ecc"], q["corrected_ecc"])
        self.assertEqual(out["metrics"]["temperature_celsius"], {"status": "unavailable"})
        self.assertEqual(out["hardware_authenticity"], "not_verified")
        self.assertEqual(out["ecc_continuity"], "not_assessed")
        self.assertFalse(out["device_sampled"])

    def test_refuses_tampered_digest_wrong_device_and_live_claims(self):
        q = fixture()
        raw = pack(q)
        with self.assertRaisesRegex(ValueError, "expected SHA-256"):
            verify(raw + b" ", DEVICE, hashlib.sha256(raw).hexdigest())
        with self.assertRaisesRegex(ValueError, "expected selection"):
            verify(raw, "0000:02:00.0")
        for field, value in (("live_query", "observed"), ("capture_identity", "verified"),
                             ("ecc_scope", "aggregate"), ("schema_version", True)):
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "scope"):
                verify(pack({**q, field: value}), DEVICE)

    def test_refuses_type_width_and_withheld_values(self):
        for field, bits in (("power_milliwatts", 32), ("corrected_ecc", 64)):
            for value in (0, "-1", "00", str(2**bits)):
                with self.subTest(field=field, value=value), self.assertRaisesRegex(ValueError, "observed"):
                    verify(pack({**fixture(), field: {"status": "observed", "value": value}}), DEVICE)
        with self.assertRaisesRegex(ValueError, "withheld"):
            verify(pack({**fixture(), "temperature_celsius": {"status": "unavailable", "value": "0"}}), DEVICE)

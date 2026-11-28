import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_redfish_rotational_capture import decode


def sample(**overrides):
    return json.dumps({"@odata.type": "#Sensor.v1_11_0.Sensor", "Id": "fan1",
                       "ReadingType": "Rotational", "ReadingUnits": "{rev}/min",
                       "Reading": 0, **overrides}).encode()


class RedfishRotational(unittest.TestCase):
    def test_accepts_only_documented_rpm_spellings(self):
        for raw, value in ((sample(), "0"), (sample(ReadingUnits="RPM", Reading=1250.5), "1250.5")):
            result = decode(raw, "fan1", hashlib.sha256(raw).hexdigest())
            self.assertEqual(result["reading"], {"status": "reported", "value_rpm": value})
            self.assertEqual(result["cooling_health"], "not_assessed")
            self.assertFalse(result["actions_performed"])

    def test_percent_velocity_and_missing_are_not_rpm(self):
        for raw, status in (
            (sample(ReadingType="Percent", ReadingUnits="%", Reading=80), "unsupported_type_or_unit"),
            (sample(ReadingType="RotationalVelocity", ReadingUnits="rad/s"), "unsupported_type_or_unit"),
            (sample(ReadingUnits="rpm"), "unsupported_type_or_unit"),
            (sample(Reading=-1), "invalid"), (sample(Reading=True), "invalid"),
            (sample(Reading=None), "unavailable"),
        ):
            self.assertEqual(decode(raw, "fan1")["reading"], {"status": status})

    def test_offline_cli_rejects_edited_capture_and_never_copies_oem(self):
        raw = sample(Reading=1250, Oem={"unused": "private-fixture"})
        p = Path(__file__).resolve().parents[1] / "scripts/check_redfish_rotational_capture.py"
        with tempfile.TemporaryDirectory() as tmp:
            capture = Path(tmp) / "fan.json"
            capture.write_bytes(raw)
            digest = hashlib.sha256(raw).hexdigest()
            args = [sys.executable, "-B", str(p), "--capture", str(capture),
                    "--sensor-id", "fan1", "--expected-sha256", digest]
            result = subprocess.run(args, cwd=tmp, check=True, capture_output=True, timeout=5)
            q = json.loads(result.stdout)
            self.assertEqual(q["reading"]["value_rpm"], "1250")
            self.assertEqual(q["live_request"], "not_performed")
            self.assertNotIn(b"private-fixture", result.stdout)
            capture.write_bytes(raw + b" ")
            refused = subprocess.run(args, cwd=tmp, capture_output=True, timeout=5)
            self.assertEqual(refused.returncode, 1)
            self.assertFalse(refused.stdout)
            self.assertIn(b"differs from expected SHA-256", refused.stderr)

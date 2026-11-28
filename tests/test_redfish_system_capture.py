import hashlib
import json
from pathlib import Path
import subprocess
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_redfish_system_capture import decode

BASE = {"@odata.type": "#ComputerSystem.v1_22_0.ComputerSystem",
        "Id": "node-1", "PowerState": "PoweringOn",
        "Status": {"Health": "OK", "HealthRollup": "Warning"}}


def blob(**kwargs):
    return json.dumps({**BASE, **kwargs}).encode()


class RedfishSavedSystem(unittest.TestCase):
    def test_file_cli_checks_selection_and_emits_only_summary(self):
        directory = Path(__file__).resolve().parent
        capture = directory / "fixtures/redfish-system-summary.json"
        script = directory.parent / "scripts/check_redfish_system_capture.py"
        digest = hashlib.sha256(capture.read_bytes()).hexdigest()
        args = [sys.executable, "-B", str(script), "--capture", str(capture), "--system-id"]
        verified = subprocess.run(args + ["node-1", "--expected-sha256", digest],
                                  capture_output=True, timeout=5, check=True)
        out = json.loads(verified.stdout)
        self.assertEqual(out["power_state"], {"status": "reported", "value": "Off"})
        self.assertEqual(out["resource_health"], {"status": "unavailable"})
        self.assertEqual(out["health_rollup"], {"status": "reported", "value": "Warning"})
        self.assertNotIn(b"synthetic-not-for-report", verified.stdout)
        self.assertNotIn(b"omit-from-summary", verified.stdout)
        wrong = subprocess.run(args + ["node-2"], capture_output=True, timeout=5, check=False)
        self.assertEqual(wrong.returncode, 1)
        self.assertFalse(wrong.stdout)
        self.assertIn(b"selected system ID", wrong.stderr)

    def test_distinguishes_power_and_two_health_scopes(self):
        out = decode(blob(), "node-1")
        self.assertEqual(out["power_state"]["value"], "PoweringOn")
        self.assertEqual(out["resource_health"]["value"], "OK")
        self.assertEqual(out["health_rollup"]["value"], "Warning")
        self.assertEqual(out["live_request"], "not_performed")
        self.assertEqual(out["physical_identity"], "not_verified")

    def test_missing_and_unsupported_are_not_healthy(self):
        out = decode(blob(PowerState=None, Status={"Health": "OEM custom"}), "node-1")
        self.assertEqual(out["resource_health"], {"status": "unsupported_value"})
        self.assertEqual(out["health_rollup"], {"status": "unavailable"})
        self.assertEqual(out["power_state"], {"status": "unavailable"})

    def test_selection_and_exact_bytes(self):
        raw = blob()
        digest = hashlib.sha256(raw).hexdigest()
        self.assertEqual(decode(raw, "node-1", digest)["digest_selection"], "matched")
        with self.assertRaisesRegex(ValueError, "selected system ID"):
            decode(raw, "node-2")
        with self.assertRaisesRegex(ValueError, "expected SHA-256"):
            decode(raw + b" ", "node-1", digest)
        with self.assertRaisesRegex(ValueError, "ComputerSystem"):
            decode(blob(**{"@odata.type": "#Chassis.v1_0_0.Chassis"}), "node-1")

    def test_omits_oem_links_actions_and_serial(self):
        raw = blob(Actions={"redacted": "do not export"}, Oem={"secret": "not for output"},
                   SerialNumber="private-serial", Links={"private": "not used"})
        out = decode(raw, "node-1")
        self.assertFalse(out["actions_performed"])
        for unwanted in ("do not export", "not for output", "private-serial"):
            self.assertNotIn(unwanted, json.dumps(out))
        with self.assertRaisesRegex(ValueError, "byte limit"):
            decode(raw + b" " * 65536, "node-1")

"""Exercise actual Hub-side planner entry points without touching a device."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import test_hub_passive_plan as fixture

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
PLANNERS = ("plan_hub_passive.py", "plan_board_bringup.py")


class PlannerCLI(unittest.TestCase):
    def run_planner(self, script, content):
        with tempfile.TemporaryDirectory() as work:
            path = Path(work) / "capabilities.json"
            dest = Path(work) / "new-report.json"
            path.write_bytes(content)
            result = subprocess.run(
                [sys.executable, "-B", str(SCRIPTS / script),
                 "--capabilities", str(path), "--run-id", fixture.TOKEN, "--evidence-path", str(dest)],
                cwd=work, capture_output=True, text=True, timeout=5,
            )
            self.assertFalse(dest.exists(), "a preview must not create an evidence file")
            return result

    def test_valid_capabilities_only_preview(self):
        content = json.dumps(fixture.PassivePlan().capabilities()).encode()
        for script in PLANNERS:
            with self.subTest(script=script):
                result = self.run_planner(script, content)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(json.loads(result.stdout)["execution"], "none")

    def test_malformed_input_has_no_plan_or_traceback(self):
        inputs = {
            "non-object": b"[]",
            "duplicate-key": b'{"contracts":{},"contracts":{}}',
            "non-finite": b'{"version":NaN}',
            "too-deep": b'{"child":' + b'[' * 2048 + b'0' + b']' * 2048 + b'}',
            "oversized": b' ' * (1024 * 1024 + 1),
        }
        for script in PLANNERS:
            for name, body in inputs.items():
                with self.subTest(script=script, name=name):
                    result = self.run_planner(script, body)
                    self.assertEqual(result.returncode, 1)
                    self.assertEqual(result.stdout, "")
                    self.assertTrue(result.stderr.startswith(("invalid:", "Preview rejected:")),
                                    result.stderr)
                    self.assertNotIn("Traceback", result.stderr)

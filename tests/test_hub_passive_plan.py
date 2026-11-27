from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from plan_hub_passive import plan

TOKEN = "0123456789abcdeffedcba9876543210"
PATH = "/tmp/controller-owned/new-report.json"


class PassivePlan(unittest.TestCase):
    def capabilities(self):
        return {"capabilities_schema_version": 1, "os_target": "linux",
                "contracts": {"passive_qualification": {
                    "observation_schema": 1, "evidence_schema": 1,
                    "run_id": "hex32", "max_evidence_bytes": 1048576,
                }, "passive_system_health": {"schema": 1, "flag": "--include-system-health"},
                "passive_observation_timeline": {"schema": 2, "flag": "--include-timeline"}}}

    def test_accepts_known_optional_only_never_executes(self):
        c = self.capabilities()
        result = plan(c, TOKEN, PATH, ["system-health", "timeline"])
        self.assertEqual(result["argv"], ["qualify", "passive", "--run-id", TOKEN,
                                         "--evidence-json", PATH, "--include-system-health"])
        self.assertEqual([x["status"] for x in result["optional"]], ["planned", "unsupported"])
        self.assertEqual(result["execution"], "none")

    def test_bad_contract_path_or_duplicate_fail(self):
        c = self.capabilities()
        for p in ("/tmp/../evil", "/tmp/test; malicious", "//tmp/evil", "relative/path"):
            with self.subTest(p=p), self.assertRaises(ValueError):
                plan(c, TOKEN, p, [])
        with self.assertRaises(ValueError):
            plan(c, TOKEN, PATH, ["timeline", "timeline"])
        c["contracts"]["passive_qualification"]["evidence_schema"] = True
        with self.assertRaises(ValueError):
            plan(c, TOKEN, PATH, [])

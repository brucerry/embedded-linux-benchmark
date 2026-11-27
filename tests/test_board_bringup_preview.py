from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from plan_board_bringup import preview  # noqa: E402


class BoardBringupTest(unittest.TestCase):
    def setUp(self):
        self.run_id = "0123456789abcdef0123456789abcdef"
        self.cap = {"capabilities_schema_version": 1, "os_target": "linux",
                    "contracts": {"passive_qualification": {
                        "observation_schema": 1, "evidence_schema": 1,
                        "run_id": "hex32", "max_evidence_bytes": 1048576}}}

    def check(self, pcie=None, edac=None):
        return preview(self.cap, self.run_id, "/tmp/new-run/result.json", pcie, edac)

    def test_old_binary_runs_inventory_without_assuming_hardware(self):
        result = self.check("0000:01:00.0=8086:1234", "mc0")
        self.assertEqual(result["execution"], "none")
        self.assertEqual(result["hardware_availability"], "not_checked")
        statuses = {r["id"]: r["status"] for r in result["steps"]}
        self.assertEqual(statuses["inventory"], "planned")
        self.assertEqual(statuses["selected_edac"], "unsupported")
        self.assertEqual(statuses["expected_pcie"], "unsupported")
        self.assertEqual(statuses["network_peer"], "skipped")
        self.assertEqual(len(result["passive_plan"]["argv"]), 6)

    def test_explicit_supported_features_remain_passive(self):
        self.cap["contracts"].update(
            passive_edac_health={"schema": 1, "flag": "--edac-controller", "scope": "selected_snapshot"},
            passive_pcie_expectation={"schema": 1, "flag": "--require-pcie"},
        )
        result = self.check("0000:01:00.0", "mc3")
        self.assertEqual(result["passive_plan"]["argv"][6:],
                         ["--require-pcie", "0000:01:00.0", "--edac-controller", "mc3"])
        self.assertEqual(self.check()["steps"][3]["status"], "not_requested")
        with self.assertRaises(ValueError):
            self.check("0000:01:00.0", "../mc3")

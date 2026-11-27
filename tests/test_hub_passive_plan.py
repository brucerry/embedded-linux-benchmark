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

    def test_explicit_pcie_requires_advertisement_and_canonical_ids(self):
        c = self.capabilities()
        request = "0000:01:00.0=1234:abcd,0000:02:00.0"
        with self.assertRaisesRegex(ValueError, "does not advertise"):
            plan(c, TOKEN, PATH, [], request)
        c["contracts"]["passive_pcie_expectation"] = {"schema": 1, "flag": "--require-pcie"}
        p = plan(c, TOKEN, PATH, [], request)
        self.assertEqual(p["argv"][-2:], ["--require-pcie", request])
        for value in ("0000:01:00.0,0000:01:00.0", "0000:01:00.0=abcd", "0000:01:20.0"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                plan(c, TOKEN, PATH, [], value)

    def test_nic_requires_explicit_interface_duration_and_capability(self):
        c = self.capabilities()
        with self.assertRaisesRegex(ValueError, "does not advertise"):
            plan(c, TOKEN, PATH, [], network_interface="eth0", network_sample_ms=100)
        c["contracts"]["passive_network_health"] = {
            "schema": 1, "max_sample_wait_ms": 200,
            "qualification_flag": "--network-health-sample-ms",
        }
        for nic, wait in (("eth0", 201), ("../eth0", 100), ("eth0", None), (None, 100)):
            with self.subTest(nic=nic, wait=wait), self.assertRaises(ValueError):
                plan(c, TOKEN, PATH, [], network_interface=nic, network_sample_ms=wait)
        result = plan(c, TOKEN, PATH, ["system-health"], network_interface="eth0", network_sample_ms=100)
        self.assertEqual(result["argv"][-5:], ["--network-interface", "eth0",
                                             "--network-health-sample-ms", "100",
                                             "--include-system-health"])
        self.assertEqual(result["execution"], "none")

    def test_thermal_window_requires_an_advertised_valid_duration(self):
        c = self.capabilities()
        with self.assertRaisesRegex(ValueError, "does not advertise"):
            plan(c, TOKEN, PATH, [], thermal_sample_ms=5)
        c["contracts"]["passive_thermal_window"] = {
            "schema": 1, "qualification_flag": "--thermal-sample-ms", "max_sample_wait_ms": 50,
        }
        self.assertEqual(plan(c, TOKEN, PATH, [], thermal_sample_ms=5)["argv"][-2:],
                         ["--thermal-sample-ms", "5"])
        for invalid in (0, True, 51, 2001):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                plan(c, TOKEN, PATH, [], thermal_sample_ms=invalid)
        self.assertNotIn("--thermal-sample-ms", plan(c, TOKEN, PATH, [])["argv"])

    def test_pcie_health_plans_one_explicit_snapshot(self):
        c = self.capabilities()
        with self.assertRaisesRegex(ValueError, "does not advertise"):
            plan(c, TOKEN, PATH, [], pcie_health_device="0000:01:00.0")
        c["contracts"]["passive_pcie_health"] = {"schema": 1, "max_sample_wait_ms": 2000}
        result = plan(c, TOKEN, PATH, [], pcie_health_device="0000:01:00.0")
        self.assertEqual(result["argv"][-2:], ["--pcie-health-device", "0000:01:00.0"])
        self.assertNotIn("--pcie-health-sample-ms", result["argv"])
        with self.assertRaisesRegex(ValueError, "invalid requested PCIe"):
            plan(c, TOKEN, PATH, [], pcie_health_device="../0000:01:00.0")

    def test_nvme_requires_named_controller_and_sysfs_only_advertisement(self):
        c = self.capabilities()
        with self.assertRaisesRegex(ValueError, "does not advertise"):
            plan(c, TOKEN, PATH, [], nvme_controller="nvme0")
        c["contracts"]["passive_nvme_controller"] = {
            "schema": 1, "flag": "--nvme-controller", "scope": "selected_sysfs_attributes_only",
        }
        for value in ("nvme01", "nvme0n1", "/dev/nvme0", "nvme00000000"):
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "invalid requested NVMe"):
                plan(c, TOKEN, PATH, [], nvme_controller=value)
        self.assertEqual(plan(c, TOKEN, PATH, [], nvme_controller="nvme0")["argv"][-2:],
                         ["--nvme-controller", "nvme0"])

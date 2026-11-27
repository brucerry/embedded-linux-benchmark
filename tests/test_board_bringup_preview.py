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

    def test_explicit_nic_and_nvme_are_only_in_plan_with_capability(self):
        def bringup(**kwargs):
            return preview(self.cap, self.run_id, "/tmp/new-run/result.json", **kwargs)

        opts = dict(network_interface="eth0", network_sample_ms=100, nvme_controller="nvme0")
        unavailable = bringup(**opts)
        statuses = {r["id"]: r["status"] for r in unavailable["steps"]}
        self.assertEqual((statuses["selected_nic"], statuses["selected_nvme"]), ("unsupported", "unsupported"))
        self.assertEqual(len(unavailable["passive_plan"]["argv"]), 6)
        self.cap["contracts"].update(
            passive_nvme_controller={"schema": 1, "flag": "--nvme-controller",
                                     "scope": "selected_sysfs_attributes_only"},
            passive_network_health={"schema": 1, "qualification_flag": "--network-health-sample-ms",
                                    "max_sample_wait_ms": 2000},
        )
        advertised = bringup(**opts)
        self.assertEqual(advertised["passive_plan"]["argv"][-6:],
                         ["--network-interface", "eth0", "--network-health-sample-ms", "100",
                          "--nvme-controller", "nvme0"])
        unrequested = {r["id"]: r["status"] for r in bringup()["steps"]}
        self.assertEqual((unrequested["selected_nic"], unrequested["selected_nvme"]),
                         ("not_requested", "not_requested"))
        with self.assertRaises(ValueError):
            bringup(network_sample_ms=100)
        with self.assertRaises(ValueError):
            bringup(nvme_controller="nvme0n1")


    def test_mmc_capability_is_explicit_and_opt_in(self):
        def make(**options):
            return preview(self.cap, self.run_id, "/tmp/new-run/result.json", **options)

        def state(result):
            return next(r["status"] for r in result["steps"] if r["id"] == "selected_mmc")

        self.assertEqual(state(make()), "not_requested")
        self.assertEqual(state(make(mmc_card="mmc0:000a")), "unsupported")
        self.assertNotIn("--mmc-card", make(mmc_card="mmc0:000a")["passive_plan"]["argv"])
        self.cap["contracts"]["passive_mmc_health"] = {
            "schema": 1, "flag": "--mmc-card", "scope": "cached_sysfs_only",
        }
        result = make(mmc_card="mmc0:000a")
        self.assertEqual(state(result), "planned")
        self.assertEqual(result["passive_plan"]["argv"][-2:], ["--mmc-card", "mmc0:000a"])
        with self.assertRaisesRegex(ValueError, "invalid requested MMC"):
            make(mmc_card="mmcblk0")


    def test_thermal_preview_states_and_kernel_value_scope(self):
        def make(wait):
            result = preview(self.cap, self.run_id, "/tmp/new-run/result.json",
                             thermal_sample_ms=wait)
            row = next(r for r in result["steps"] if r["id"] == "selected_thermal_window")
            return result, row

        self.assertEqual(make(None)[1]["status"], "not_requested")
        self.assertEqual(make(5)[1]["status"], "unsupported")
        self.cap["contracts"]["passive_thermal_window"] = {
            "schema": 1, "qualification_flag": "--thermal-sample-ms", "max_sample_wait_ms": 10,
        }
        self.assertEqual(make(11)[1]["status"], "unsupported")
        result, selected = make(5)
        self.assertEqual(result["execution"], "none")
        self.assertEqual(result["passive_plan"]["argv"][-2:], ["--thermal-sample-ms", "5"])
        self.assertEqual(selected, {"id": "selected_thermal_window", "status": "planned",
                                   "requested_ms": 5, "reading_scope": "kernel_reported",
                                   "sensor_provenance": "not_verified"})
        for bad in (True, 0, 2001):
            with self.subTest(bad=bad), self.assertRaisesRegex(ValueError, "invalid requested thermal"):
                make(bad)


    def test_selected_pcie_health_does_not_claim_an_observation(self):
        def make(device):
            result = preview(self.cap, self.run_id, "/tmp/new-run/result.json",
                             pcie_health_device=device)
            step = next(r for r in result["steps"] if r["id"] == "selected_pcie_health")
            return result, step

        self.assertEqual(make(None)[1]["status"], "not_requested")
        self.assertEqual(make("0000:01:00.0")[1]["status"], "unsupported")
        self.cap["contracts"]["passive_pcie_health"] = {"schema": 1, "max_sample_wait_ms": 2000}
        previewed, selected = make("0000:01:00.0")
        self.assertEqual(previewed["execution"], "none")
        self.assertEqual(previewed["hardware_availability"], "not_checked")
        self.assertEqual(previewed["passive_plan"]["argv"][-2:],
                         ["--pcie-health-device", "0000:01:00.0"])
        self.assertEqual(selected, {"id": "selected_pcie_health", "status": "planned",
                                   "device": "0000:01:00.0", "scope": "single_passive_snapshot",
                                   "hardware_authenticity": "not_verified"})
        with self.assertRaisesRegex(ValueError, "invalid requested PCIe health"):
            make("pci0000:01")


    def test_pcie_window_preview_does_not_fall_back_to_single_snapshot(self):
        device = "0000:01:00.0"
        self.cap["contracts"]["passive_pcie_health"] = {"schema": 1, "max_sample_wait_ms": 10}

        def make(wait, selected=device):
            result = preview(self.cap, self.run_id, "/tmp/new-run/result.json",
                             pcie_health_device=selected, pcie_health_sample_ms=wait)
            row = next(r for r in result["steps"] if r["id"] == "selected_pcie_health")
            return result, row

        planned, row = make(10)
        self.assertEqual(row, {"id": "selected_pcie_health", "status": "planned",
                               "device": device, "scope": "two_passive_snapshots",
                               "hardware_authenticity": "not_verified", "requested_ms": 10})
        self.assertEqual(planned["passive_plan"]["argv"][-4:],
                         ["--pcie-health-device", device, "--pcie-health-sample-ms", "10"])
        unsupported, row = make(11)
        self.assertEqual(row["status"], "unsupported")
        self.assertNotIn("--pcie-health-device", unsupported["passive_plan"]["argv"])
        for bad, selected in ((True, device), (0, device), (2001, device), (5, None)):
            with self.subTest(bad=bad, selected=selected), self.assertRaises(ValueError):
                make(bad, selected)

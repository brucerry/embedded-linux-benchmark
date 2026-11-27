import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
from compare_pcie_snapshots import DEVICE_KEYS, ROOT_KEYS, compare  # noqa: E402

TARGET = "0000:02:00.0"
PARENT = "0000:01:00.0"


def fixture(at, count):
    absent = {"status": "unavailable"}
    total = {"status": "observed", "value": {
        "total_messages": str(count), "categories": {"RxErr": str(count)}}}
    rows = [{"bdf": bdf, "role": role, "driver_binding": {"status": "unbound"},
             "link": {}, "device_aer": dict(zip(DEVICE_KEYS, (total, absent, absent)))}
            for bdf, role in ((TARGET, "target"), (PARENT, "upstream"))]
    return {"schema_version": 1, "kind": "pcie_saved_snapshot",
            "boot_id": "00000000-0000-4000-8000-000000000001",
            "started_uptime_cs": str(at), "finished_uptime_cs": str(at + 1),
            "observer_identity": [{"bdf": row["bdf"], "sysfs_dev": "1", "sysfs_inode": str(i + 1)}
                                  for i, row in enumerate(rows)],
            "snapshot": {"schema_version": 1, "device": TARGET, "observations": rows,
                         "root_hierarchy_aer": [{"bdf": PARENT, "counters":
                                                 dict(zip(ROOT_KEYS, (
                                                     {"status": "observed", "value": str(count)},
                                                     absent, absent)))}]}}


def encode(obj):
    return json.dumps(obj).encode()


class PcieSnapshotTest(unittest.TestCase):
    def setUp(self):
        self.before = fixture(1, 2**63 + 1)
        self.after = fixture(5, 2**63 + 4)

    def current(self):
        return compare(encode(self.before), encode(self.after))

    def test_exact_increments_by_observer_and_unavailable_distinct(self):
        result = self.current()
        self.assertEqual(result["hardware_authenticity"], "not_verified")
        rows = result["by_observer"]
        self.assertEqual([r["bdf"] for r in rows], [TARGET, PARENT])
        self.assertEqual(rows[0]["device_aer"][DEVICE_KEYS[0]]["value"]["total_messages"], "3")
        self.assertEqual(rows[0]["root_hierarchy_aer"][ROOT_KEYS[0]], {"status": "unavailable"})
        self.assertEqual(rows[1]["root_hierarchy_aer"][ROOT_KEYS[0]]["value"], "3")
        self.assertNotIn("boot_id", result)

    def test_changed_continuity_and_out_of_order(self):
        baseline = copy.deepcopy(self.after)
        variants = (
            ("boot_changed", lambda x: x.update(boot_id=x["boot_id"][:-1] + "2")),
            ("windows_not_strictly_ordered", lambda x: x.update(started_uptime_cs="2")),
            ("observer_identity_changed", lambda x: x["observer_identity"][0].update(sysfs_inode="9")),
            ("driver_binding_changed", lambda x: x["snapshot"]["observations"][0].update(
                driver_binding={"status": "changed"})),
            ("driver_binding_indeterminate", lambda x: x["snapshot"]["observations"][0].update(
                driver_binding={"status": "inaccessible"})),
        )
        for reason, update in variants:
            self.after = copy.deepcopy(baseline)
            update(self.after)
            got = self.current()
            self.assertEqual(got["reason"], reason)
            self.assertNotIn("by_observer", got)

    def test_decreased_changed_set_and_missing(self):
        self.after["snapshot"]["observations"][0]["device_aer"] = {
            DEVICE_KEYS[0]: {"status": "observed", "value": {
                "total_messages": "1", "categories": {"RxErr": "1"}}},
            DEVICE_KEYS[1]: {"status": "observed", "value": {
                "total_messages": "1", "categories": {}}},
            DEVICE_KEYS[2]: {"status": "unavailable"},
        }
        self.after["snapshot"]["observations"][1]["device_aer"][DEVICE_KEYS[0]] = {
            "status": "observed", "value": {"total_messages": "1", "categories": {"BadTLP": "1"}}}
        target, parent = self.current()["by_observer"]
        self.assertEqual(target["device_aer"][DEVICE_KEYS[0]]["reason"], "counter_decreased")
        self.assertEqual(target["device_aer"][DEVICE_KEYS[1]]["reason"], "observation_state_changed")
        self.assertEqual(parent["device_aer"][DEVICE_KEYS[0]]["reason"], "category_set_changed")

    def test_cli_exit_and_rejection(self):
        with tempfile.TemporaryDirectory() as root:
            a, b = Path(root) / "a.json", Path(root) / "b.json"
            a.write_bytes(encode(self.before))
            b.write_bytes(encode(self.after))
            run = lambda: subprocess.run(
                [sys.executable, "-B", str(SCRIPTS / "compare_pcie_snapshots.py"), str(a), str(b)],
                capture_output=True, timeout=5)
            self.assertEqual(run().returncode, 0)
            b.write_bytes(encode(self.before))
            self.assertEqual(run().returncode, 2)
            b.write_bytes(b'{"schema_version":1,"schema_version":1}')
            self.assertEqual(run().returncode, 1)
            self.after["observer_identity"][0]["sysfs_inode"] = str(2**64)
            b.write_bytes(encode(self.after))
            self.assertEqual(run().returncode, 1)

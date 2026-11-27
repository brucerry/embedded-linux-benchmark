import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import subprocess
import sys
import unittest

spec = importlib.util.spec_from_file_location(
    "check_hub_evidence", Path(__file__).resolve().parents[1] / "scripts/check_hub_evidence.py"
)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

TOKEN = "0123456789abcdeffedcba9876543210"


def pack(q, elapsed=10):
    return json.dumps({
        "evidence_schema_version": 1, "kind": "passive_qualification",
        "started_unix_ms": 100, "finished_unix_ms": 90, "elapsed_monotonic_ms": elapsed,
        "qualification": q,
    }, ensure_ascii=False).encode()


def fixture():
    return {
        "qualification_schema_version": 1, "kind": "passive_qualification_observation",
        "scope": "bring-up", "workload_execution": "none", "run_id": TOKEN,
        "inventory": {},
        "pcie_health": {
            "before": {"unnamed_bits": {}, "wide": "9007199254740993"},
            "after": {"unnamed_bits": {"bit_29": "1"}, "wide": "9007199254740994"},
            "delta": {"by_observer": [{"bdf": "0000:01:00.0",
                       "device_aer": {"aer_dev_correctable": {
                           "status": "not_comparable", "reason": "category_set_changed"
                       }}}]},
        },
    }


class Evidence(unittest.TestCase):
    def test_optional_mmc_preserves_unavailable_and_checks_raw_shape(self):
        q = fixture()
        q["mmc_health"] = {"schema_version": 1, "card": "mmc0:0001",
                           "status": "unavailable"}
        self.assertEqual(m.verify(pack(q), json.dumps(q).encode(), TOKEN)["status"], "correlated")
        q["mmc_health"]["life_time_raw"] = {"status": "observed", "value": 0}
        with self.assertRaisesRegex(ValueError, "invalid withheld MMC"):
            m.verify(pack(q), json.dumps(q).encode(), TOKEN)

    def test_released_single_network_snapshot_is_validated_not_just_correlated(self):
        from check_network_health_window import KEYS

        q = fixture()
        sample = {
            "schema_version": 1, "interface": "eth0", "status": "observed",
            "carrier": {"status": "observed", "value": True},
            "speed_mbps": {"status": "unavailable"},
            "duplex": {"status": "unavailable"},
            "operstate": {"status": "observed", "value": "up"},
            "counters": {key: {"status": "unavailable"} for key in KEYS},
        }
        sample["counters"]["rx_bytes"] = {"status": "observed", "value": "9007199254740993"}
        sample["counters"]["tx_errors"] = {"status": "observed", "value": "0"}
        for valid in (sample, {"schema_version": 1, "interface": "eth0", "status": "unavailable"}):
            q["network_health"] = valid
            result = m.verify(pack(q), json.dumps(q).encode(), TOKEN)
            self.assertEqual(result["hardware_authenticity"], "not_verified")
        rounded = copy.deepcopy(sample)
        rounded["counters"]["rx_bytes"]["value"] = 9007199254740993
        invented = copy.deepcopy(sample)
        invented["counters"]["rx_errors"]["value"] = "0"
        for invalid in (None, [], {**sample, "interface": "../eth0"}, rounded, invented):
            with self.subTest(invalid=invalid):
                q["network_health"] = invalid
                with self.assertRaises(ValueError):
                    m.verify(pack(q), json.dumps(q).encode(), TOKEN)

    def test_keeps_sparse_rows_and_counter_precision(self):
        q = fixture()
        encoded = pack(q)
        result = m.verify(encoded, json.dumps(q).encode(), TOKEN)
        self.assertEqual(result["status"], "correlated")
        self.assertEqual(result["hardware_authenticity"], "not_verified")
        self.assertEqual(m.parse(encoded)["qualification"], q)
        self.assertEqual(len(result["sha256"]), 64)
        # Replacing the unknown interval with zero or discarding the raw before/after is detectable.
        after = copy.deepcopy(q)
        after["pcie_health"]["delta"]["by_observer"][0]["device_aer"]["aer_dev_correctable"] = {
            "status": "observed", "value": "0"
        }
        with self.assertRaisesRegex(ValueError, "differs"):
            m.verify(encoded, json.dumps(after).encode(), TOKEN)

    def test_rejects_altered_run_and_json_type(self):
        q = fixture()
        with self.assertRaisesRegex(ValueError, "run ID mismatch"):
            m.verify(pack(q), json.dumps(q).encode(), "1" * 32)
        changed = copy.deepcopy(q)
        changed["qualification_schema_version"] = True
        with self.assertRaisesRegex(ValueError, "unsupported"):
            m.verify(pack(q), json.dumps(changed).encode(), TOKEN)

    def test_checks_pressure_series_windows_even_when_both_captures_match(self):
        q = fixture()
        snapshot = {"schema_version": 1, "scope": "system_proc_pressure",
                    **{field: {"status": "unavailable"} for field in ("cpu", "memory", "io")}}
        series = {"schema_version": 1, "kind": "linux_pressure_series", "scope": "single_process",
                  "sample_count": 2, "requested_gap_ms": 100, "workload_execution": "none",
                  "samples": [{"index": i, "started_monotonic_ms": i * 102,
                               "finished_monotonic_ms": i * 102 + 2, "pressure": snapshot}
                              for i in range(2)]}
        q["system_health"] = {"pressure_series": series}
        def run():
            return m.verify(pack(q, elapsed=104), json.dumps(q).encode(), TOKEN)
        self.assertEqual(run()["status"], "correlated")
        for obj, field, value in (
            (series, "sample_count", 3), (series, "requested_gap_ms", 99),
            (series["samples"][0], "index", False),
            (series["samples"][1], "started_monotonic_ms", 101),
            (series["samples"][1], "finished_monotonic_ms", 105),
            (snapshot, "scope", "host_timestamp"),
        ):
            with self.subTest(field=field), self.assertRaises(ValueError):
                old = obj[field]
                try:
                    obj[field] = value
                    run()
                finally:
                    obj[field] = old

    def test_rejects_matching_network_windows_with_conflicting_metadata(self):
        q = fixture()
        window = {"schema_version": 1, "mode": "two_passive_snapshots", "requested_wait_ms": 5,
                  "sample_start_interval_ms": 5, "continuity": "unknown",
                  "before": {"schema_version": 1, "interface": "lo", "status": "unavailable"},
                  "after": {"schema_version": 1, "interface": "lo", "status": "unavailable"},
                  "delta": {"status": "not_comparable"}}
        q["network_health"] = window

        def run():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(run()["status"], "correlated")
        for obj, field, value in (
            (window, "sample_start_interval_ms", 4),
            (window, "sample_start_interval_ms", 11),
            (window, "requested_wait_ms", True),
            (window["after"], "interface", "eth0"),
            (window["delta"], "status", "observed_differences"),
        ):
            with self.subTest(field=field), self.assertRaises(ValueError):
                old = obj[field]
                try:
                    obj[field] = value
                    run()
                finally:
                    obj[field] = old

    def test_nvme_scope_and_controller_binding(self):
        q = fixture()
        q.pop("pcie_health")
        nvme = {"schema_version": 1, "controller": "nvme0", "status": "unavailable", "smart_sampled": False}
        q["nvme_health"] = nvme
        argv = ["qualify", "passive", "--run-id", TOKEN, "--evidence-json", "/tmp/test/report",
                "--nvme-controller", "nvme0"]
        plan = {"plan_schema_version": 1, "argv": argv, "execution": "none",
                "max_evidence_bytes": 1048576, "optional": []}
        def run():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN, plan)

        self.assertEqual(run()["status"], "correlated")
        nvme["controller"] = "nvme1"
        with self.assertRaisesRegex(ValueError, "differs from trusted plan"):
            run()
        nvme["controller"] = "nvme0"
        nvme["smart_sampled"] = True
        with self.assertRaisesRegex(ValueError, "invalid passive NVMe"):
            run()

    def test_matching_network_envelopes_cannot_hide_forged_deltas(self):
        from test_network_health_window import fixture as nic_fixture
        q = fixture()
        window = nic_fixture()
        window.update(requested_wait_ms=5, sample_start_interval_ms=5)
        q["network_health"] = window
        def run():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(run()["status"], "correlated")
        window["delta"].pop("comparable_counters")
        window["delta"].pop("counter_coverage")
        self.assertEqual(run()["status"], "correlated")  # older collectors omitted coverage
        window["delta"]["counters"]["rx_bytes"]["value"] = "0"
        with self.assertRaisesRegex(ValueError, "delta disagrees"):
            run()  # altered identically in stdout and saved evidence

    def test_rejects_overlaps_and_unreported_observation_timeline_phases(self):
        q = fixture()
        timeline = {"schema_version": 1, "clock": "single_process_monotonic",
                    "execution": "sequential",
                    "phases": [
                        {"phase": "pcie_health", "started_monotonic_ms": 0, "finished_monotonic_ms": 5},
                        {"phase": "inventory", "started_monotonic_ms": 5, "finished_monotonic_ms": 10},
                    ]}
        q["observation_timeline"] = timeline

        def run():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(run()["status"], "correlated")
        for obj, field, value in (
            (timeline, "phases", timeline["phases"][:-1]),
            (timeline, "phases", list(reversed(timeline["phases"]))),
            (timeline["phases"][1], "started_monotonic_ms", 4),
            (timeline["phases"][1], "finished_monotonic_ms", 11),
            (timeline["phases"][0], "phase", "nvme_health"),
        ):
            with self.subTest(field=field), self.assertRaises(ValueError):
                old = obj[field]
                try:
                    obj[field] = value
                    run()
                finally:
                    obj[field] = old

    def test_thermal_window_arithmetic_and_missing_sensors(self):
        q = fixture()
        def reading(value):
            return {"status": "observed", "value": value}
        def zone(i, value):
            return {"zone": f"thermal_zone{i}", "type": reading("reference"),
                    "temperature_millicelsius": reading(value)}
        before = {"status": "observed", "zones": [zone(0, "1200"), zone(1, "0")]}
        after = {"status": "observed", "zones": [zone(0, "-300")]}
        window = {
            "requested_wait_ms": 3, "sample_start_interval_ms": 4,
            "before": before, "after": after,
            "differences": [
                {"zone": "thermal_zone0", "difference_millicelsius": {
                    "status": "observed_difference", "value": "-1500"
                }},
                {"zone": "thermal_zone1", "difference_millicelsius": {"status": "not_comparable"}},
            ],
        }
        q["inventory"] = {"thermal": copy.deepcopy(before), "thermal_window": window}
        def run():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)
        self.assertEqual(run()["status"], "correlated")
        for obj, field, value in (
            (window, "sample_start_interval_ms", 11),
            (window, "requested_wait_ms", True),
            (window["differences"][0]["difference_millicelsius"], "value", "-1499"),
            (window["differences"][1], "difference_millicelsius",
             {"status": "observed_difference", "value": "0"}),
            (after["zones"][0]["type"], "value", "replacement"),
            (after["zones"][0]["temperature_millicelsius"], "value", "-273151"),
            (q["inventory"], "thermal", after),
        ):
            with self.subTest(field=field), self.assertRaises(ValueError):
                old = obj[field]
                try:
                    obj[field] = value
                    run()
                finally:
                    obj[field] = old

    def test_binds_optional_observations_to_saved_passive_plan(self):
        q = fixture()
        del q["pcie_health"]
        q["system_health"] = {"available": False}
        plan = {
            "plan_schema_version": 1, "execution": "none", "max_evidence_bytes": m.MAX_BYTES,
            "argv": ["qualify", "passive", "--run-id", TOKEN, "--evidence-json",
                     "/tmp/report.json", "--include-system-health"],
            "optional": [{"feature": "system-health", "status": "planned"},
                         {"feature": "timeline", "status": "unsupported"}],
        }
        def run(observation, saved_plan):
            return m.verify(pack(observation), json.dumps(observation).encode(), TOKEN, saved_plan)
        self.assertEqual(run(q, plan)["status"], "correlated")
        for key, value in (
            ("max_evidence_bytes", 64), ("plan_schema_version", True),
            ("argv", [*plan["argv"], "--include-timeline"]), ("argv", plan["argv"][:-1]),
            ("optional", [plan["optional"][0]] * 2),
        ):
            with self.subTest(key=key), self.assertRaises(ValueError):
                run(q, {**plan, key: value})
        for observation in (
            {k: v for k, v in q.items() if k != "system_health"},
            {**q, "system_health": None},
            {**q, "pcie_health": {}},
        ):
            with self.assertRaisesRegex(ValueError, "disagree with plan"):
                run(observation, plan)
        with self.assertRaisesRegex(ValueError, "unplanned thermal"):
            run({**q, "inventory": {"thermal_window": {
                "requested_wait_ms": 1, "sample_start_interval_ms": 1,
                "before": {"status": "unavailable", "zones": []},
                "after": {"status": "unavailable", "zones": []}, "differences": [],
            }, "thermal": {"status": "unavailable", "zones": []}}}, plan)

    def test_receipt_binds_exact_plan_bytes_only_when_supplied(self):
        q = {k: v for k, v in fixture().items() if k != "pcie_health"}
        plan = {"plan_schema_version": 1, "execution": "none",
                "max_evidence_bytes": m.MAX_BYTES, "optional": [],
                "argv": ["qualify", "passive", "--run-id", TOKEN,
                         "--evidence-json", "/tmp/report.json"]}
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            evidence, stdout, saved = (root / name for name in ("evidence", "stdout", "plan"))
            evidence.write_bytes(pack(q))
            stdout.write_text(json.dumps(q), encoding="utf-8")
            plan_data = (json.dumps(plan, indent=2) + "\n").encode()
            saved.write_bytes(plan_data)
            command = [sys.executable, "-B", m.__file__, "--evidence", str(evidence),
                       "--stdout", str(stdout), "--run-id", TOKEN]
            legacy = subprocess.run(command, capture_output=True, check=True, timeout=4)
            self.assertNotIn("plan_sha256", json.loads(legacy.stdout))
            receipt = subprocess.run(command + ["--plan", str(saved)], capture_output=True,
                                     check=True, timeout=4)
            self.assertEqual(json.loads(receipt.stdout)["plan_sha256"],
                             hashlib.sha256(plan_data).hexdigest())

    def test_required_pcie_ids_must_agree_with_reported_status(self):
        q = fixture()
        q["board_check"] = "see_required_pcie"
        ids = {"vendor": "1234", "device": "abcd"}
        match = {"bdf": "0000:01:00.0", "status": "match", "expected": ids, "observed": ids}
        def verify_rows(rows):
            q["inventory"]["required_pcie"] = rows
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)
        self.assertEqual(verify_rows([match])["status"], "correlated")
        self.assertEqual(verify_rows([{**match, "status": "missing", "observed": None}])
                         ["status"], "correlated")
        for rows in ([], [match, match], [{**match, "status": "mismatch"}],
                     [{**match, "status": "missing"}],
                     [{"bdf": match["bdf"], "status": "match"}]):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                verify_rows(rows)

    def test_planned_pcie_expectations_bind_order_and_requested_ids(self):
        q = fixture()
        del q["pcie_health"]
        q["board_check"] = "see_required_pcie"
        ids = {"vendor": "1234", "device": "abcd"}
        rows = [{"bdf": "0000:01:00.0", "status": "match", "expected": ids, "observed": ids},
                {"bdf": "0000:02:00.0", "status": "missing"}]
        plan = {"plan_schema_version": 1, "execution": "none", "max_evidence_bytes": m.MAX_BYTES,
                "argv": ["qualify", "passive", "--run-id", TOKEN, "--evidence-json",
                         "/tmp/report.json", "--require-pcie", "0000:01:00.0=1234:abcd,0000:02:00.0"],
                "optional": []}
        def verify_rows(actual):
            q["inventory"]["required_pcie"] = actual
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN, plan)
        self.assertEqual(verify_rows(rows)["status"], "correlated")
        for changed in (rows[::-1], rows[:1], [{**rows[0], "bdf": "0000:03:00.0"}, rows[1]]):
            with self.subTest(changed=changed), self.assertRaisesRegex(ValueError, "disagree with plan"):
                verify_rows(changed)

    def test_rejects_duplicate_truncated_and_nonfinite(self):
        for data in (b'{"a":1,"a":2}', b'{"a":', b'{"a":NaN}', b' ' * (m.MAX_BYTES + 1)):
            with self.subTest(data=data[:20]), self.assertRaises(ValueError):
                m.parse(data)

    def test_local_capture_reader_refuses_symlinks_and_oversized_files(self):
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp)
            target = p / "data"
            link = p / "link"
            target.write_bytes(b"{}")
            link.symlink_to(target)
            self.assertEqual(m.read_bounded(target), b"{}")
            with self.assertRaises(OSError):
                m.read_bounded(link)
            target.write_bytes(b" " * (m.MAX_BYTES + 1))
            with self.assertRaises(ValueError):
                m.read_bounded(target)


if __name__ == "__main__":
    unittest.main()

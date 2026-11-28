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
    def test_planned_pcie_window_recomputes_delta_and_binds_duration(self):
        from compare_pcie_snapshots import compare
        from test_pcie_snapshot_comparison import TARGET, fixture as capture, encode
        from plan_hub_passive import plan
        from test_hub_passive_plan import PassivePlan
        cap = PassivePlan().capabilities()
        cap["contracts"]["passive_pcie_health"] = {"schema": 1, "max_sample_wait_ms": 2000}
        a, b = capture(1, 2**63), capture(5, 2**63 + 3)
        rows = compare(encode(a), encode(b))["by_observer"]
        q = fixture()
        q["pcie_health"] = {"schema_version": 1, "device": TARGET, "requested_wait_ms": 5,
                            "sample_start_interval_ms": 6, "before": a["snapshot"],
                            "after": b["snapshot"], "delta": {"status": "compared", "by_observer": rows}}
        args = (cap, TOKEN, "/tmp/owned/snapshot.json", [])
        selected = plan(*args, pcie_health_device=TARGET, pcie_health_sample_ms=5)

        def validate(expected=selected):
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN, expected)

        self.assertEqual(validate()["hardware_authenticity"], "not_verified")
        with self.assertRaisesRegex(ValueError, "window differs from trusted plan"):
            validate(plan(*args, pcie_health_device=TARGET, pcie_health_sample_ms=6))
        q["pcie_health"]["sample_start_interval_ms"] = 11
        with self.assertRaisesRegex(ValueError, "invalid PCIe health window timing"):
            validate()
        q["pcie_health"]["sample_start_interval_ms"] = 6
        rows[0]["device_aer"]["aer_dev_correctable"]["value"]["total_messages"] = "0"
        with self.assertRaisesRegex(ValueError, "delta disagrees"):
            validate()
        q["pcie_health"]["delta"] = {"status": "not_comparable",
                                      "reason": "boot_identity_unavailable"}
        self.assertEqual(validate()["status"], "correlated")

    def test_planned_pcie_health_binds_target_and_preserves_observer_scope(self):
        from test_pcie_snapshot_comparison import TARGET, PARENT, fixture as snapshot
        from plan_hub_passive import plan
        from test_hub_passive_plan import PassivePlan
        cap = PassivePlan().capabilities()
        cap["contracts"]["passive_pcie_health"] = {"schema": 1, "max_sample_wait_ms": 2000}
        q = fixture()
        q["pcie_health"] = snapshot(1, 0)["snapshot"]
        selected = plan(cap, TOKEN, "/tmp/owned/evidence.json", [], pcie_health_device=TARGET)
        self.assertEqual(m.verify(pack(q), json.dumps(q).encode(), TOKEN, selected)["status"],
                         "correlated")
        wrong = plan(cap, TOKEN, "/tmp/owned/evidence.json", [], pcie_health_device=PARENT)
        with self.assertRaisesRegex(ValueError, "differs from trusted plan"):
            m.verify(pack(q), json.dumps(q).encode(), TOKEN, wrong)
        q["pcie_health"]["observations"][1]["role"] = "target"
        with self.assertRaisesRegex(ValueError, "invalid PCIe health observer"):
            m.verify(pack(q), json.dumps(q).encode(), TOKEN, selected)

    def test_planned_pcie_link_fields_match_the_producer(self):
        from test_pcie_snapshot_comparison import TARGET, fixture as capture
        from plan_hub_passive import plan
        from test_hub_passive_plan import PassivePlan
        cap = PassivePlan().capabilities()
        cap["contracts"]["passive_pcie_health"] = {"schema": 1, "max_sample_wait_ms": 2000}
        for endpoint in ("single", "before", "after"):
            with self.subTest(endpoint=endpoint):
                q = fixture()
                snapshot = capture(1, 0)["snapshot"]
                if endpoint == "single":
                    q["pcie_health"] = snapshot
                    wait = None
                else:
                    q["pcie_health"] = {
                        "schema_version": 1, "device": TARGET, "requested_wait_ms": 5,
                        "sample_start_interval_ms": 6, "before": copy.deepcopy(snapshot),
                        "after": copy.deepcopy(snapshot),
                        "delta": {"status": "not_comparable", "reason": "boot_identity_unavailable"},
                    }
                    snapshot = q["pcie_health"][endpoint]
                    wait = 5
                selected = plan(cap, TOKEN, "/tmp/owned/evidence.json", [], pcie_health_device=TARGET,
                                pcie_health_sample_ms=wait)
                observer = snapshot["observations"][1]
                valid = {
                    "current_link_speed": {"status": "observed", "value": "future vendor  format"},
                    "current_link_width": {"status": "observed", "value": "~" * 128},
                    "max_link_speed": {"status": "invalid"},
                    "max_link_width": {"status": "unavailable"},
                }

                def validate(link):
                    observer["link"] = link
                    return m.verify(pack(q), json.dumps(q).encode(), TOKEN, selected)

                self.assertEqual(validate(valid)["status"], "correlated")
                invalid = [{k: v for k, v in valid.items() if k != "max_link_speed"},
                           dict(valid, extra={"status": "unavailable"}), [],
                           dict(valid, max_link_width={"status": "unavailable", "value": "0"})]
                for raw in (False, 4, "", " 4", "4 ", "4\tGT/s", "４", "x" * 129):
                    invalid.append(dict(valid, current_link_width={"status": "observed", "value": raw}))
                for link in invalid:
                    with self.subTest(link=link), self.assertRaisesRegex(ValueError, "invalid PCIe link"):
                        validate(link)

    def test_edac_preserves_observed_zero_u64_and_missing_state(self):
        q = fixture()
        report = {"schema_version": 1, "kind": "linux_edac_memory", "controller": "mc0",
                  "source": "linux_sysfs", "result": {
                      "status": "observed", "seconds_since_reset": {"status": "unavailable"},
                      "counters": {"ce_count": {"status": "observed", "value": str(2**64 - 1)},
                                   "ue_count": {"status": "observed", "value": "0"}},
                  }}
        q["edac_health"] = report

        def validate():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(validate()["status"], "correlated")
        for value in (0, "00", str(2**64)):
            report["result"]["counters"]["ue_count"]["value"] = value
            with self.subTest(value=value), self.assertRaisesRegex(ValueError, "EDAC observation value"):
                validate()
        report["result"] = {"status": "unavailable"}
        self.assertEqual(validate()["status"], "correlated")
        report["result"]["counters"] = {"ue_count": {"status": "observed", "value": "0"}}
        with self.assertRaisesRegex(ValueError, "invalid withheld EDAC result"):
            validate()

    def test_cpu_frequency_does_not_replace_hardware_value_with_scaling_estimate(self):
        q = fixture()
        row = {"policy": "policy0", "scaling_reported_khz": {"status": "observed", "value": "1200000"},
               "cpuinfo_current_khz": {"status": "unavailable"},
               "policy_max_khz": {"status": "observed", "value": "2000000"},
               "cpuinfo_max_khz": {"status": "inaccessible"}}
        report = {"status": "observed", "policies": [row]}
        q["inventory"]["cpu_frequency"] = report

        def check_report():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(check_report()["hardware_authenticity"], "not_verified")
        for bad in (
            dict(report, policies=[row, row]),
            dict(report, status="no_policies"),
            dict(report, policies=[dict(row, policy="policy01")]),
            dict(report, policies=[dict(row, scaling_reported_khz={"status": "observed", "value": "0"})]),
            dict(report, policies=[dict(row, cpuinfo_current_khz={"status": "unavailable",
                                                                "value": "1200000"})]),
        ):
            with self.assertRaises(ValueError):
                q["inventory"]["cpu_frequency"] = bad
                check_report()

    def test_cpu_throttle_rows_are_observations_not_a_package_aggregate(self):
        q = fixture()
        zero = {"status": "observed", "value": "0"}
        wide = {"status": "observed", "value": "9007199254740993"}
        rows = [{"cpu": num, "core_count": zero, "package_count_at_cpu": wide}
                for num in (0, 24)]
        report = {"status": "observed", "cpu_observers": rows}
        q["inventory"]["cpu_thermal_throttle"] = report

        def check_report():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(check_report()["status"], "correlated")
        variants = (
            dict(report, status="unavailable"),
            dict(report, cpu_observers=[rows[1], rows[0]]),
            dict(report, cpu_observers=[rows[0], rows[0]]),
            dict(report, cpu_observers=[dict(rows[0], cpu=True)]),
            dict(report, package_total="18014398509481986"),
            dict(report, cpu_observers=[dict(rows[0], package_count_at_cpu={
                "status": "unavailable", "value": "0",
            })]),
        )
        for bad in variants:
            with self.assertRaises(ValueError):
                q["inventory"]["cpu_thermal_throttle"] = bad
                check_report()
        q["inventory"]["cpu_thermal_throttle"] = {"status": "unavailable", "cpu_observers": []}
        self.assertEqual(check_report()["hardware_authenticity"], "not_verified")

    def test_powercap_keeps_units_and_legacy_reports_distinct(self):
        q = fixture()
        row = {"zone": "intel-rapl:0:1", "name": {"status": "observed", "value": "CPU core"},
               "energy_microjoules": {"status": "observed", "value": "18446744073709551615"},
               "max_energy_range_microjoules": {"status": "unavailable"}}
        cap = {"status": "observed", "zones": [row]}
        q["inventory"]["powercap"] = cap

        def check_report():
            return m.verify(pack(q), json.dumps(q).encode(), TOKEN)

        self.assertEqual(check_report()["status"], "correlated")
        row.update(power_microwatts={"status": "observed", "value": "0"},
                   max_power_range_microwatts={"status": "inaccessible"})
        self.assertEqual(check_report()["hardware_authenticity"], "not_verified")
        for changed in ({"status": "observed", "value": True},
                        {"status": "observed", "value": "18446744073709551616"},
                        {"status": "unavailable", "value": "0"}):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                cap["zones"] = [dict(row, power_microwatts=changed)]
                check_report()
        for bad in (dict(cap, status="no_zones", zones=[row]),
                    dict(cap, zones=[row, row]),
                    dict(cap, zones=[dict(row, zone="../intel-rapl:0")])):
            with self.assertRaises(ValueError):
                q["inventory"]["powercap"] = bad
                check_report()

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

    def test_checks_pressure_measurements_without_rounding_or_inventing_cpu_full(self):
        some = {"status": "observed", "avg10_percent": "0.00", "avg60_percent": "99.99",
                "avg300_percent": "100.00", "total_stall_us": str(2**64 - 1)}
        for series in (False, True):
            q = fixture()
            snapshot = {"schema_version": 1, "scope": "system_proc_pressure",
                        "cpu": {"status": "observed", "some": copy.deepcopy(some),
                                "full": {"status": "not_applicable"}},
                        "memory": {"status": "observed", "some": copy.deepcopy(some),
                                   "full": {"status": "unavailable"}},
                        "io": {"status": "inaccessible"}}
            q["system_health"] = {"pressure_series": {
                "schema_version": 1, "kind": "linux_pressure_series", "scope": "single_process",
                "sample_count": 2, "requested_gap_ms": 100, "workload_execution": "none",
                "samples": [{"index": i, "started_monotonic_ms": i * 102,
                             "finished_monotonic_ms": i * 102 + 2, "pressure": snapshot}
                            for i in range(2)],
            }} if series else {"pressure": snapshot}

            def run():
                return m.verify(pack(q, elapsed=104), json.dumps(q).encode(), TOKEN)

            self.assertEqual(run()["status"], "correlated")
            cases = [(snapshot["cpu"], "full", some),
                     (snapshot["memory"], "some", {"status": "unavailable"}),
                     (snapshot, "io", {"status": "inaccessible", "total_stall_us": "0"}),
                     (snapshot["memory"]["some"], "avg10_percent", "100.01"),
                     (snapshot["memory"]["some"], "avg10_percent", "00.10"),
                     (snapshot["memory"]["some"], "avg10_percent", 0),
                     (snapshot["memory"]["some"], "total_stall_us", str(2**64))]
            for container, field, value in cases:
                with self.subTest(series=series, field=field, value=value):
                    old = container[field]
                    try:
                        container[field] = value
                        with self.assertRaisesRegex(ValueError, "pressure|CPU full"):
                            run()
                    finally:
                        container[field] = old

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

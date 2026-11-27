import copy
import importlib.util
import json
from pathlib import Path
import tempfile
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
                  "before": {"interface": "lo"}, "after": {"interface": "lo"},
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

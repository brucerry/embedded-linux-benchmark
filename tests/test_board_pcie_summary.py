import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from test_pcie_requirements import FIRST, pair
from summarize_board_pcie import summarize
from check_board_batch_coverage import verify as coverage


def manifest(rows):
    return json.dumps({"schema_version": 1, "boards": rows}).encode()


class BoardPcieSummaryTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.path = Path(tmp.name)

    def artifacts(self, board, token_char, status, identity=None):
        observed = {"bdf": FIRST, "status": status}
        if identity:
            observed.update(expected=identity, observed=identity)
        evidence, stdout, token, plan, process_exit = pair([observed])
        run_id = token_char * 32
        hashes = {}
        for kind, initial in {"evidence": evidence, "stdout": stdout, "plan": plan}.items():
            data = initial.replace(token.encode(), run_id.encode())
            (self.path / f"{run_id}.{kind}.json").write_bytes(data)
            hashes[kind] = hashlib.sha256(data).hexdigest()
        return {"board": board, "run_id": run_id, "process_exit": process_exit, "sha256": hashes}

    def test_results_stay_separate_and_no_yield(self):
        rows = [self.artifacts("fixture-a", "a", "present"),
                self.artifacts("fixture-b", "b", "missing"),
                self.artifacts("fixture-c", "c", "unavailable")]
        result = summarize(manifest(rows), self.path)
        self.assertEqual([(r["board"], r["result"]["outcome"]) for r in result["boards"]],
                         [("fixture-a", "pass"), ("fixture-b", "fail"), ("fixture-c", "unavailable")])
        self.assertEqual(result["manufacturing_yield"], "not_assessed")
        self.assertEqual(result["comparison"]["groups"], [])
        self.assertEqual(result["comparison"]["unbound_boards"], ["fixture-a", "fixture-b", "fixture-c"])

    def test_repeated_board_run_and_raw_evidence(self):
        first = self.artifacts("fixture-a", "a", "present")
        for second, error in [
            (dict(self.artifacts("fixture-b", "b", "present"), board="FIXTURE-A"), "board label"),
            (dict(first, board="fixture-b"), "run ID"),
            (dict(first, board="fixture-b", run_id="b" * 32), "evidence digest"),
        ]:
            with self.subTest(error=error), self.assertRaisesRegex(ValueError, error):
                summarize(manifest([first, second]), self.path)

    def test_tampering_or_nonzero_exit_rejects_batch(self):
        first = self.artifacts("fixture-a", "a", "present")
        path = self.path / ("a" * 32 + ".plan.json")
        with self.assertRaisesRegex(ValueError, "successful exit"):
            summarize(manifest([dict(first, process_exit=1)]), self.path)
        path.write_bytes(path.read_bytes() + b" ")
        with self.assertRaisesRegex(ValueError, "plan digest mismatch"):
            summarize(manifest([first]), self.path)

    def test_no_paths_in_manifest_empty_or_oversized(self):
        for rows in ([], [None] * 17):
            with self.assertRaisesRegex(ValueError, "1 to 16"):
                summarize(manifest(rows), self.path)
        row = self.artifacts("fixture-a", "a", "present")
        with self.assertRaisesRegex(ValueError, "run ID"):
            summarize(manifest([dict(row, run_id="../escape")]), self.path)

    def with_context(self, board="fixture-a", char="a", engine="e", device=None, status=None):
        identity = dict(vendor="8086", device=device) if device else None
        row = self.artifacts(board, char, status or ("match" if device else "present"), identity)
        row["expected_engine_sha256"] = engine * 64
        self.context_record(row, "profile", {"schema_version": 1, "profile_id": "rev-a",
                                             "require_pcie": FIRST + ("=8086:" + device if device else "")})
        self.context_record(row, "capture", {"schema_version": 1, "board": row["board"],
                                             "run_id": row["run_id"], "process_exit": 0,
                                             "engine_sha256": engine * 64})
        return row

    def context_record(self, row, kind, data):
        raw = json.dumps(data).encode()
        (self.path / f'{row["run_id"]}.{kind}.json').write_bytes(raw)
        row["sha256"][kind] = hashlib.sha256(raw).hexdigest()

    def summarize_v2(self, *rows):
        return summarize(json.dumps({"schema_version": 2, "boards": list(rows)}).encode(), self.path)

    def test_v2_reports_distinct_profile_and_binary_identity(self):
        result = self.summarize_v2(self.with_context())
        context = result["boards"][0]["context"]
        self.assertEqual(result["schema_version"], 2)
        self.assertEqual(context["profile_id"], "rev-a")
        self.assertEqual(context["engine_sha256"], "e" * 64)
        self.assertEqual(context["runtime_attestation"], "not_verified")

    def test_v2_rejects_wrong_expected_binary_or_stale_context(self):
        row = self.with_context()
        with self.assertRaisesRegex(ValueError, "expected asset"):
            self.summarize_v2(dict(row, expected_engine_sha256="f" * 64))
        with self.assertRaisesRegex(ValueError, "capture context"):
            self.summarize_v2(dict(row, board="fixture-b"))

    def test_v2_rejects_matching_digest_for_wrong_profile(self):
        row = self.with_context()
        self.context_record(row, "profile", {"schema_version": 1, "profile_id": "rev-a",
                                             "require_pcie": FIRST + "=8086:1234"})
        with self.assertRaisesRegex(ValueError, "differs from board profile"):
            self.summarize_v2(row)


    def test_comparison_never_joins_same_label_with_different_required_device(self):
        a = self.with_context(device="1234")
        b = self.with_context(board="fixture-b", char="b", device="4321")
        result = self.summarize_v2(a, b)
        self.assertTrue(all(row["result"]["outcome"] == "pass" for row in result["boards"]))
        groups = result["comparison"]["groups"]
        self.assertEqual({group["profile_id"] for group in groups}, {"rev-a"})
        self.assertEqual(len({group["profile_sha256"] for group in groups}), 2)
        self.assertEqual({tuple(group["boards"]) for group in groups}, {("fixture-a",), ("fixture-b",)})

    def test_exact_profile_reuse_groups_only_same_engine_digest(self):
        a = self.with_context()
        b = self.with_context(board="fixture-b", char="b")
        c = self.with_context(board="fixture-c", char="c", engine="f")
        groups = self.summarize_v2(a, b, c)["comparison"]["groups"]
        self.assertEqual(len({group["profile_sha256"] for group in groups}), 1)
        self.assertEqual({group["engine_sha256"]: group["boards"] for group in groups},
                         {"e" * 64: ["fixture-a", "fixture-b"], "f" * 64: ["fixture-c"]})


    def coverage(self, rows, roster):
        packet = json.dumps({"schema_version": 2, "boards": rows}).encode()
        plan = json.dumps({"schema_version": 1, "boards": roster}).encode()
        return coverage(packet, self.path, plan, hashlib.sha256(plan).hexdigest())

    @staticmethod
    def expected(row):
        return dict(board=row["board"], profile_sha256=row["sha256"]["profile"],
                    engine_sha256=row["expected_engine_sha256"])

    def test_coverage_distinguishes_missing_capture_from_pcie_outcomes(self):
        a = self.with_context()
        b = self.with_context(board="fixture-b", char="b", status="missing")
        c = self.with_context(board="fixture-c", char="c", status="unavailable")
        d = self.with_context(board="fixture-d", char="d")
        roster = [self.expected(r) for r in (a, b, c, d)]
        result = self.coverage([a, b, c], roster)
        self.assertEqual(result["counts"],
                         {"pass": 1, "fail": 1, "unavailable": 1, "not_assessed": 0, "no_capture": 1})
        self.assertEqual(result["boards"][-1], {"board": "fixture-d",
                                            "evidence_outcome": "no_capture", "run_id": None})
        self.assertEqual(result["manufacturing_yield"], "not_assessed")
        self.assertEqual(self.coverage([], roster)["counts"]["no_capture"], 4)

    def test_coverage_refuses_unlisted_board_wrong_revision_or_wrong_roster_digest(self):
        a = self.with_context()
        b = self.with_context(board="fixture-b", char="b")
        expected = self.expected(a)
        with self.assertRaisesRegex(ValueError, "not in the roster"):
            self.coverage([a, b], [expected])
        with self.assertRaisesRegex(ValueError, "differs from expected"):
            self.coverage([a], [dict(expected, profile_sha256="f" * 64)])
        with self.assertRaisesRegex(ValueError, "duplicate expected board"):
            self.coverage([], [expected, dict(expected, board="FIXTURE-A")])
        with self.assertRaisesRegex(ValueError, "roster differs"):
            coverage(b"{}", self.path, b"{}", "0" * 64)

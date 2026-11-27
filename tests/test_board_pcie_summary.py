import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from test_pcie_requirements import FIRST, pair
from summarize_board_pcie import summarize


def manifest(rows):
    return json.dumps({"schema_version": 1, "boards": rows}).encode()


class BoardPcieSummaryTest(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.path = Path(tmp.name)

    def artifacts(self, board, token_char, status):
        evidence, stdout, token, plan, process_exit = pair([{"bdf": FIRST, "status": status}])
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

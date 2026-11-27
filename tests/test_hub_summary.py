import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from summarize_hub_evidence import summarize
from test_hub_evidence import fixture, pack


def entry(host, digit, rows=None):
    q = fixture()
    token = digit * 32
    q["run_id"] = token
    if rows is not None:
        q["inventory"]["required_pcie"] = rows
    return host, token, pack(q), json.dumps(q).encode()


class Summary(unittest.TestCase):
    def test_keeps_same_bdf_separate_by_host(self):
        a = {"bdf": "0000:01:00.0", "status": "mismatch"}
        b = dict(a, status="unavailable")
        result = summarize([entry("host-a", "1", [a]), entry("host-b", "2", [b]),
                            entry("host-c", "3")])
        self.assertEqual(result["source_identity"], "not_verified")
        r = result["records_by_host"]
        self.assertEqual(r[0]["counts"]["mismatch"], 1)
        self.assertEqual(r[1]["counts"]["mismatch"], 0)
        self.assertEqual(r[1]["counts"]["unavailable"], 1)
        self.assertEqual(r[2]["status"], "no_expectation_rows")

    def test_bad_or_replayed_runs_fail_closed(self):
        original = entry("host", "1")
        with self.assertRaisesRegex(ValueError, "repeated run"):
            summarize([original, ("other",) + original[1:]])
        with self.assertRaisesRegex(ValueError, "host label"):
            summarize([original, entry("host", "2")])
        with self.assertRaisesRegex(ValueError, "PCIe row"):
            summarize([entry("host", "1", [{"bdf": "0000:00:01.0", "status": "healthy"}])])
        with self.assertRaises(ValueError):
            summarize([original] * 9)

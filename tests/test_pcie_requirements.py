import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from evaluate_pcie_requirements import evaluate  # noqa: E402
from test_hub_evidence import fixture, pack, TOKEN  # noqa: E402

FIRST = "0000:01:00.0"
SECOND = "0000:02:00.0"


def pair(rows=None):
    q = fixture()
    q.pop("pcie_health")
    argv = ["qualify", "passive", "--run-id", TOKEN, "--evidence-json", "/tmp/fixture/report.json"]
    if rows is not None:
        q["board_check"] = "see_required_pcie"
        q["inventory"]["required_pcie"] = rows
        specs = [r["bdf"] + ("=" + r["expected"]["vendor"] + ":" + r["expected"]["device"]
                             if "expected" in r else "") for r in rows]
        argv += ["--require-pcie", ",".join(specs)]
    plan = {"plan_schema_version": 1, "argv": argv, "execution": "none",
            "max_evidence_bytes": 1048576, "optional": []}
    return pack(q), json.dumps(q).encode(), TOKEN, json.dumps(plan).encode(), 0


class PcieRequirementsTest(unittest.TestCase):
    def test_absent_plan_rows_do_not_generate_a_pass(self):
        self.assertEqual(evaluate(*pair())["outcome"], "not_assessed")

    def test_pass_fail_and_unavailable_are_separate(self):
        ok = {"bdf": FIRST, "status": "present"}
        unknown = {"bdf": SECOND, "status": "unavailable"}
        result = evaluate(*pair([ok, unknown]))
        self.assertEqual(result["outcome"], "unavailable")
        self.assertEqual(result["counts"], {"pass": 1, "fail": 0, "unavailable": 1})
        self.assertEqual(result["rows"][0]["requirement"], "presence_only")
        self.assertEqual(evaluate(*pair([ok]))["outcome"], "pass")
        self.assertEqual(evaluate(*pair([dict(ok, status="missing"), unknown]))["outcome"], "fail")
        ids = {"vendor": "8086", "device": "1234"}
        matched = dict(ok, status="match", expected=ids, observed=ids)
        result = evaluate(*pair([matched]))
        self.assertEqual(result["rows"][0]["requirement"], "device_identity")
        self.assertEqual(result["production_qualification"], "not_assessed")

    def test_binding_and_process_failure_fail_closed(self):
        args = list(pair([{"bdf": FIRST, "status": "present"}]))
        args[-1] = 1
        with self.assertRaisesRegex(ValueError, "successful exit"):
            evaluate(*args)
        args[-1] = 0
        plan = json.loads(args[3])
        plan["argv"][-1] = SECOND
        args[3] = json.dumps(plan).encode()
        with self.assertRaisesRegex(ValueError, "disagree"):
            evaluate(*args)

import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_cpufreq_policy_evidence import FIELDS, verify


def sample():
    fields = {key: {"status": "unavailable"} for key in FIELDS}
    fields["scaling_cur_freq"] = {"status": "observed", "value": "1200000"}
    return dict(schema_version=1, kind="cpufreq_policy", policy="policy0", status="observed",
                unit="kHz", fields=fields)


class CPUFreqEvidence(unittest.TestCase):
    def test_keeps_unavailable_hardware_distinct_from_driver_value(self):
        raw = json.dumps(sample()).encode()
        out = verify(raw, "policy0", hashlib.sha256(raw).hexdigest())
        self.assertEqual(out["fields"]["cpuinfo_cur_freq"], {"status": "unavailable"})
        self.assertEqual(out["fields"]["scaling_cur_freq"]["value"], "1200000")
        self.assertEqual(out["scaling_current_scope"], "driver_reported")
        self.assertEqual(out["throttling"], "not_assessed")

    def test_rejects_wrong_policy_unit_and_values(self):
        emit = lambda row: json.dumps(row).encode()
        for row, policy, digest in ((sample(), "policy1", None),
                                   (dict(sample(), unit="Hz"), "policy0", None),
                                   (dict(sample(), status="changed"), "policy0", None),
                                   (sample(), "policy0", "0" * 64)):
            with self.assertRaises(ValueError):
                verify(emit(row), policy, digest)
        for value in ("0", 100, "0100", "-1", "18446744073709551616"):
            report = sample()
            report["fields"]["scaling_cur_freq"]["value"] = value
            with self.assertRaises(ValueError):
                verify(emit(report), "policy0")

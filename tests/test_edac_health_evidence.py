import hashlib
import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_edac_health_evidence import verify


def report(result):
    return json.dumps({
        "schema_version": 1, "kind": "linux_edac_memory", "source": "linux_sysfs",
        "controller": "mc0", "result": result,
    }).encode()


class EdacEvidence(unittest.TestCase):
    def test_exact_selected_counts(self):
        result = {
            "status": "observed", "seconds_since_reset": {"status": "invalid"},
            "counters": {"ce_count": {"status": "observed", "value": str(2**64 - 1)},
                         "ue_count": {"status": "observed", "value": "0"}},
        }
        raw = report(result)
        digest = hashlib.sha256(raw).hexdigest()
        out = verify(raw, "mc0", digest)
        self.assertEqual(out["result"], result)
        self.assertEqual(out["digest_selection"], "matched")
        self.assertEqual(out["hardware_authenticity"], "not_verified")
        self.assertEqual(out["memory_health"], "not_assessed")

    def test_unavailable_has_no_invented_value(self):
        result = {"status": "unavailable"}
        out = verify(report(result), "mc0")
        self.assertEqual(out["result"], result)
        self.assertEqual(out["digest_selection"], "unbound")

    def test_wrong_controller_digest_or_non_snapshot_fail(self):
        raw = report({"status": "unavailable"})
        with self.assertRaisesRegex(ValueError, "differs from expected selection"):
            verify(raw, "mc1")
        with self.assertRaisesRegex(ValueError, "differs from expected SHA-256"):
            verify(raw + b" ", "mc0", hashlib.sha256(raw).hexdigest())
        with self.assertRaisesRegex(ValueError, "invalid expected EDAC controller"):
            verify(raw, "mc00")
        expanded = json.loads(raw)
        expanded["sample_wait_ms"] = 100
        with self.assertRaisesRegex(ValueError, "invalid EDAC selected observation"):
            verify(json.dumps(expanded).encode(), "mc0")

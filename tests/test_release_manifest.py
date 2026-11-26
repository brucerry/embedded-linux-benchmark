"""Portable policy checks for the Linux asset manifest tooling."""
import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "release_manifest.py"
SPEC = importlib.util.spec_from_file_location("release_manifest", SCRIPT)
MANIFEST = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MANIFEST)


class ManifestTest(unittest.TestCase):
    def test_glibc_maximum_is_numeric_and_uses_version_needs(self):
        report = (
            "Version definition section '.gnu.version_d':\n  Name: GLIBC_9.0\n"
            "Version needs section '.gnu.version_r':\n  Name: GLIBC_2.9\n"
            "  Name: GLIBC_2.34\n  Name: GCC_4.2.0\n"
        )
        self.assertEqual(MANIFEST.glibc_floor(report), "2.34")
        self.assertIsNone(MANIFEST.glibc_floor("Name: GLIBC_3.0"))

    def test_canonical_ordering_and_unchanged_manifest(self):
        data = MANIFEST.canonical({"b": 2, "a": 1})
        self.assertEqual(data, b'{\n  "a": 1,\n  "b": 2\n}\n')
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "manifest.json"
            path.write_bytes(data)
            MANIFEST.verify(path, data)
            path.write_bytes(data + b" ")
            with self.assertRaisesRegex(ValueError, "manifest mismatch"):
                MANIFEST.verify(path, data)

    def test_refuses_symlink_and_wrong_elf(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "fixture"
            path.write_bytes(b"not ELF")
            alias = path.with_name("alias")
            alias.symlink_to(path)
            with self.assertRaises(OSError):
                MANIFEST.open_regular(alias, 64)
            with self.assertRaisesRegex(ValueError, "unexpected ELF"):
                MANIFEST.inspect(path, MANIFEST.TARGETS[0])

    def test_rejects_unbounded_or_malformed_source_metadata(self):
        with self.assertRaisesRegex(ValueError, "version tag"):
            MANIFEST.build_manifest(Path("unused"), "nonsense", "0" * 40)
        with self.assertRaisesRegex(ValueError, "full lowercase Git commit"):
            MANIFEST.build_manifest(Path("unused"), "v0.1.2", "abc")


if __name__ == "__main__":
    unittest.main()

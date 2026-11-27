import copy
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from plan_engine_asset import ASSETS, LOADER, asset_preview


def fixture():
    rows = []
    for arch, (name, triple, libc) in ASSETS.items():
        dynamic = arch == "x86_64"
        rows.append({
            "architecture": arch, "file": name, "os": "linux", "executable": "eld-engine",
            "bytes": 1024, "sha256": "a" * 64, "verification": "binary_inspected",
            "qualified_targets": [], "declared_rust_target": triple, "declared_build_libc": libc,
            "linkage": "dynamic" if dynamic else "static", "runtime_libc": "glibc" if dynamic else None,
            "interpreter": LOADER if dynamic else None, "min_direct_glibc": "2.34" if dynamic else None,
            "needed": ["libc.so.6", "libgcc_s.so.1"] if dynamic else [],
        })
    return {"schema_version": 1, "project": "embedded-linux-diagnostic-engine",
            "source_commit": "f" * 40, "version": "v0.3.0", "assets": rows}


class AssetPreview(unittest.TestCase):
    def test_static_metadata_is_not_qualification(self):
        result = asset_preview(fixture(), "linux", "aarch64")
        self.assertEqual(result["status"], "metadata_candidate")
        self.assertEqual(result["asset"]["file"], "eld-engine-linux-aarch64-musl")
        self.assertEqual(result["provenance"], "not_verified")
        self.assertEqual(result["deployment"], "not_authorized")
        self.assertEqual(result["qualification"], "not_declared")

    def test_dynamic_requires_confirmed_facts_and_numeric_floor(self):
        data = fixture()
        libs = ["libc.so.6", "libgcc_s.so.1"]
        for glibc, loader, available, expect in (
            (None, LOADER, libs, "missing_runtime_facts"),
            ("2.9", LOADER, libs, "glibc_below_direct_floor"),
            ("2.34.0", "/lib/ld-musl-x86_64.so.1", libs, "incompatible_loader"),
            ("2.34.0", LOADER, libs[:1], "missing_direct_library_confirmation"),
            ("2.34.0", LOADER, libs, "metadata_candidate"),
        ):
            with self.subTest(expect=expect):
                result = asset_preview(data, "linux", "x86_64", glibc, loader, available)
                self.assertEqual(result["status"], expect)
                self.assertEqual(result["asset"] is None, expect != "metadata_candidate")
        self.assertEqual(asset_preview(data, "darwin", "aarch64")["status"], "unsupported_target")

    def test_rejects_invalid_entries(self):
        for key, value in (("file", "../eld-engine"), ("bytes", True),
                           ("qualified_targets", ["unreviewed-board"]),
                           ("runtime_libc", "musl"), ("sha256", "?")):
            with self.subTest(field=key), self.assertRaises(ValueError):
                data = fixture()
                data["assets"][0][key] = value
                asset_preview(data, "linux", "aarch64")
        data = fixture()
        data["assets"][1] = copy.deepcopy(data["assets"][0])
        with self.assertRaises(ValueError):
            asset_preview(data, "linux", "aarch64")

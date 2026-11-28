import hashlib
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

from test_engine_asset_plan import fixture

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_preflight_archive import MANIFEST, verify

VERSION = "v0.3.0-preflight.40"
COMMIT = "f" * 40


def write_bundle(path, edit=None):
    manifest = fixture()
    manifest["version"] = VERSION
    files = []
    for row in manifest["assets"]:
        content = row["architecture"].encode()
        row.update(bytes=len(content), sha256=hashlib.sha256(content).hexdigest())
        files.append((row["file"], content, 0o755))
    files.append((MANIFEST, json.dumps(manifest).encode(), 0o600))
    if edit:
        edit(files)
    with tarfile.open(path, "w", format=tarfile.USTAR_FORMAT) as archive:
        for name, content, mode in files:
            member = tarfile.TarInfo(name)
            member.mode = mode
            member.size = len(content)
            archive.addfile(member, io.BytesIO(content))
    return hashlib.sha256(path.read_bytes()).hexdigest()


class PreflightArchive(unittest.TestCase):
    def test_exact_archive_selection_is_not_release_authority(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "preflight.tar"
            sha = write_bundle(path)
            out = verify(path, sha, VERSION, COMMIT)
            self.assertEqual(len(out["members"]), 3)
            self.assertEqual(out["execution"], "none")
            self.assertEqual(out["deployment"], "not_authorized")
            self.assertFalse(out["qualified"])
            with self.assertRaisesRegex(ValueError, "SHA-256 differs"):
                verify(path, "0" * 64, VERSION, COMMIT)
            with self.assertRaisesRegex(ValueError, "source or version"):
                verify(path, sha, VERSION, "a" * 40)

    def test_rejects_extra_duplicate_mutated_and_wrong_modes(self):
        with tempfile.TemporaryDirectory() as d:
            path = Path(d) / "preflight.tar"
            for change, reason in (
                (lambda xs: xs.append(("other", b"x", 0o755)), "unexpected"),
                (lambda xs: xs.append(xs[0]), "duplicate"),
                (lambda xs: xs.__setitem__(0, (xs[0][0], b"other", 0o755)), "differs from manifest"),
                (lambda xs: xs.__setitem__(0, (xs[0][0], xs[0][1], 0o600)), "mode or length"),
            ):
                with self.subTest(reason=reason), self.assertRaisesRegex(ValueError, reason):
                    verify(path, write_bundle(path, change), VERSION, COMMIT)

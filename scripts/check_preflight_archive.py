#!/usr/bin/env python3
"""Check a locally saved Engine preflight tar without extraction or execution."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import tarfile

from check_hub_evidence import parse
from plan_engine_asset import asset_preview
from release_manifest import MAX_ASSET, MAX_MANIFEST, MANIFEST, TARGETS, identity, open_regular, require

MAX_TAR = 2 * MAX_ASSET + MAX_MANIFEST + 65536
NAMES = {entry[0] for entry in TARGETS}
SHA = re.compile(r"[a-f0-9]{64}")
VERSION = re.compile(r"v[0-9]+\.[0-9]+\.[0-9]+-preflight(?:\.[0-9]+)?")


def digest(stream, maximum):
    h, size = hashlib.sha256(), 0
    for block in iter(lambda: stream.read(65536), b""):
        size += len(block)
        require(size <= maximum, "preflight bytes exceed limit")
        h.update(block)
    return h.hexdigest(), size


def verify(path, expected_sha256, version, commit):
    require(type(expected_sha256) is str and SHA.fullmatch(expected_sha256),
            "expected a lowercase SHA-256 digest")
    require(type(version) is str and len(version) <= 64 and VERSION.fullmatch(version),
            "expected a private preflight version")
    require(type(commit) is str and re.fullmatch(r"[a-f0-9]{40}", commit), "invalid source commit")
    stream, before = open_regular(path, MAX_TAR)
    with stream:
        measured, size = digest(stream, MAX_TAR)
        require(size == before.st_size and measured == expected_sha256, "archive SHA-256 differs")
        stream.seek(0)
        members = {}
        manifest = None
        with tarfile.open(fileobj=stream, mode="r:") as archive:
            for item in archive:
                name = item.name
                cap = MAX_MANIFEST if name == MANIFEST else MAX_ASSET
                require(name in NAMES | {MANIFEST} and name not in members,
                        "unexpected or duplicate archive member")
                expected_mode = 0o600 if name == MANIFEST else 0o755
                require(item.type == tarfile.REGTYPE and not item.pax_headers and not item.issparse()
                        and item.mode == expected_mode and 0 < item.size <= cap,
                        "invalid archive member type, mode or length")
                with archive.extractfile(item) as data:
                    if name == MANIFEST:
                        raw = data.read(cap + 1)
                        require(len(raw) == item.size, "short manifest member")
                        manifest = parse(raw)
                        h, n = hashlib.sha256(raw).hexdigest(), len(raw)
                    else:
                        h, n = digest(data, cap)
                require(n == item.size, "short archive member")
                members[name] = {"bytes": n, "sha256": h}
        require(members.keys() == NAMES | {MANIFEST}, "incomplete archive")
        require(type(manifest) is dict and
                (manifest.get("version"), manifest.get("source_commit")) == (version, commit),
                "manifest source or version differs")
        # Validate the complete manifest in its existing closed asset-selection path.
        asset_preview(manifest, "linux", "aarch64")
        for row in manifest["assets"]:
            member = members[row["file"]]
            require(member == {"bytes": row["bytes"], "sha256": row["sha256"]},
                    "binary member differs from manifest")
        require(identity(before) == identity(os.fstat(stream.fileno())), "archive changed during inspection")
    return {"schema_version": 1, "kind": "engine_preflight_archive_check",
            "archive_sha256": measured, "source_commit": commit, "version": version,
            "members": members, "provenance": "not_verified", "qualified": False,
            "deployment": "not_authorized", "execution": "none"}


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--archive", required=True, type=Path)
    cli.add_argument("--expected-sha256", required=True)
    cli.add_argument("--version", required=True)
    cli.add_argument("--commit", required=True)
    args = cli.parse_args()
    try:
        out = verify(args.archive, args.expected_sha256, args.version, args.commit)
    except (OSError, ValueError, RecursionError, tarfile.TarError) as exc:
        cli.exit(1, f"Preflight archive refused: {exc}\n")
    print(json.dumps(out, sort_keys=True))


if __name__ == "__main__":
    main()

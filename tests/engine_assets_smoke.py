"""Check preview metadata against the two inspected preflight assets."""
import argparse
import hashlib
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from check_hub_evidence import check, parse, read_bounded  # noqa: E402
from plan_engine_asset import asset_preview  # noqa: E402
from release_manifest import MANIFEST, MAX_ASSET, open_regular  # noqa: E402


def smoke(directory):
    manifest = parse(read_bounded(directory / MANIFEST))
    check(asset_preview(manifest, "linux", "x86_64")["status"] == "missing_runtime_facts",
          "preview guessed an x86-64 runtime")
    for entry in manifest["assets"]:
        cpu = entry["architecture"]
        # A fixture that supplies the required runtime facts from the manifest;
        # this deliberately does not attest a target or launch a binary.
        preview = asset_preview(manifest, "linux", cpu, entry["min_direct_glibc"],
                                entry["interpreter"], entry["needed"])
        check(preview["status"] == "metadata_candidate"
              and preview["deployment"] == "not_authorized"
              and preview["execution"] == "none", "unexpected asset preview status")
        result = preview["asset"]
        stream, st = open_regular(directory / result["file"], MAX_ASSET)
        with stream:
            digest = hashlib.sha256()
            total = 0
            while True:
                block = stream.read(65536)
                if not block:
                    break
                total += len(block)
                check(total <= MAX_ASSET, "asset exceeded byte limit")
                digest.update(block)
        check(total == st.st_size == result["bytes"] and digest.hexdigest() == result["sha256"],
              "selected asset does not match preflight bytes")
    print("Engine metadata candidates match both preflight assets; no deployment authorized")


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("assets", type=Path, help="fresh, locally built preflight asset directory")
    smoke(cli.parse_args().assets)

"""Compare Engine asset metadata with reported target facts. Never deploy."""
import argparse
import json
import re

from check_hub_evidence import check, parse, read_bounded
from release_manifest import MAX_ASSET, MAX_MANIFEST, TARGETS

ASSETS = {arch: (name, triple, libc) for name, arch, triple, libc, _ in TARGETS}
LOADER = "/lib64/ld-linux-x86-64.so.2"
LIB_NAME = re.compile(r"[A-Za-z0-9._+-]{1,64}")
VERSION = re.compile(r"[0-9]{1,4}(?:\.[0-9]{1,4}){1,2}")


def version(value):
    check(type(value) is str and VERSION.fullmatch(value), "invalid glibc version")
    parts = [int(p) for p in value.split(".")]
    return tuple(parts + [0] * (3 - len(parts)))


def asset_preview(manifest, os_target, arch, glibc=None, loader=None, libraries=()):
    check(type(manifest) is dict and type(manifest.get("schema_version")) is int
          and manifest["schema_version"] == 1
          and manifest.get("project") == "embedded-linux-diagnostic-engine"
          and type(manifest.get("source_commit")) is str
          and re.fullmatch(r"[0-9a-f]{40}", manifest["source_commit"])
          and type(manifest.get("version")) is str
          and re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?",
                           manifest["version"]) and len(manifest["version"]) <= 64,
          "unsupported Engine manifest")
    rows = manifest.get("assets")
    check(type(rows) is list and len(rows) == len(ASSETS), "invalid asset collection")
    assets = {}
    for row in rows:
        check(type(row) is dict and type(row.get("architecture")) is str
              and row["architecture"] in ASSETS and row["architecture"] not in assets,
              "invalid or repeated asset architecture")
        machine = row["architecture"]
        name, triple, libc = ASSETS[machine]
        check(row.get("file") == name and row.get("executable") == "eld-engine"
              and row.get("os") == "linux" and row.get("declared_rust_target") == triple
              and row.get("declared_build_libc") == libc
              and type(row.get("bytes")) is int and 0 < row["bytes"] <= MAX_ASSET
              and type(row.get("sha256")) is str and re.fullmatch(r"[0-9a-f]{64}", row["sha256"])
              and row.get("verification") == "binary_inspected"
              and row.get("qualified_targets") == [], "invalid inspected asset entry")
        needed = row.get("needed")
        check(type(needed) is list and len(needed) <= 32
              and all(type(n) is str and LIB_NAME.fullmatch(n) for n in needed)
              and len(set(needed)) == len(needed), "invalid direct library list")
        if machine == "aarch64":
            check(row.get("linkage") == "static" and row.get("interpreter") is None
                  and row.get("min_direct_glibc") is None and row.get("runtime_libc") is None
                  and not needed, "invalid static asset metadata")
        else:
            check(row.get("linkage") == "dynamic" and row.get("interpreter") == LOADER
                  and row.get("runtime_libc") == "glibc" and "libc.so.6" in needed,
                  "invalid dynamic asset metadata")
            version(row.get("min_direct_glibc"))
        assets[machine] = row

    result = {"preview_schema_version": 1, "execution": "none", "provenance": "not_verified",
              "qualification": "not_declared", "deployment": "not_authorized", "asset": None}
    reason = None
    if os_target != "linux" or arch not in assets:
        reason = "unsupported_target"
    elif arch == "x86_64":
        row = assets[arch]
        if not glibc or not loader:
            reason = "missing_runtime_facts"
        elif loader != LOADER:
            reason = "incompatible_loader"
        elif version(glibc) < version(row["min_direct_glibc"]):
            reason = "glibc_below_direct_floor"
        elif not set(row["needed"]).issubset(libraries):
            reason = "missing_direct_library_confirmation"
    if reason:
        return {**result, "status": reason}
    row = assets[arch]
    return {**result, "status": "metadata_candidate",
            "asset": {k: row[k] for k in ("file", "sha256", "bytes")}}


def staging_preview(asset, facts):
    """Evaluate supplied staging observations, not their trustworthiness."""
    check(type(facts) is dict and set(facts) == {
        "private", "writable", "mount_executable", "available_bytes"
    }, "invalid staging observations")
    for key in ("private", "writable", "mount_executable"):
        check(facts[key] is None or type(facts[key]) is bool, "invalid " + key)
    free = facts["available_bytes"]
    check(free is None or (type(free) is int and 0 <= free <= (1 << 63) - 1),
          "invalid available_bytes")
    if asset is None:
        return "no_metadata_candidate"
    for key, reason in (("private", "not_private"), ("writable", "not_writable"),
                        ("mount_executable", "noexec_mount")):
        if facts[key] is False:
            return reason
    if free is not None and free < asset["bytes"]:
        return "insufficient_space_for_asset"
    if any(value is None for value in facts.values()):
        return "missing_staging_facts"
    return "observed_staging_requirements_met"


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--manifest", required=True)
    cli.add_argument("--target-os", required=True)
    cli.add_argument("--target-arch", required=True)
    cli.add_argument("--glibc")
    cli.add_argument("--loader")
    cli.add_argument("--library", action="append", default=[])
    cli.add_argument("--staging-facts", help="optional JSON observations for the selected staging directory")
    args = cli.parse_args()
    try:
        raw = read_bounded(args.manifest)
        check(len(raw) <= MAX_MANIFEST, "manifest exceeds byte limit")
        result = asset_preview(parse(raw), args.target_os, args.target_arch,
                               args.glibc, args.loader, args.library)
        if args.staging_facts:
            raw = read_bounded(args.staging_facts)
            check(len(raw) <= 4096, "staging observations exceed byte limit")
            result["staging_status"] = staging_preview(result["asset"], parse(raw))
    except (OSError, ValueError, RecursionError) as exc:
        cli.exit(1, f"invalid asset metadata: {exc}\n")
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()

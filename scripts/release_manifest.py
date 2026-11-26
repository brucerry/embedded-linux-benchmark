#!/usr/bin/env python3
"""Generate or verify an inspected, unsigned diagnostic engine asset manifest."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys

MAX_ASSET = 64 * 1024 * 1024
MAX_MANIFEST = 32 * 1024
MANIFEST = "eld-engine-release-manifest.json"
TARGETS = (
    ("eld-engine-linux-aarch64-musl", "aarch64", "aarch64-unknown-linux-musl", "musl", 183),
    ("eld-engine-linux-x86_64-gnu", "x86_64", "x86_64-unknown-linux-gnu", "glibc", 62),
)


def require(value, message):
    if not value:
        raise ValueError(message)


def identity(info):
    return (
        info.st_dev, info.st_ino, info.st_mode, info.st_size,
        info.st_mtime_ns, info.st_ctime_ns,
    )


def open_regular(path, max_size):
    flags = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK
    fd = os.open(path, flags)
    try:
        info = os.fstat(fd)
        require(
            stat.S_ISREG(info.st_mode) and 0 < info.st_size <= max_size,
            f"{path.name}: expected a bounded regular file",
        )
        return os.fdopen(fd, "rb"), info
    except BaseException:
        os.close(fd)
        raise


def readelf(stream, option):
    tool = "/usr/bin/readelf"
    fd = stream.fileno()
    result = subprocess.run(
        [tool, "--wide", option, f"/proc/self/fd/{fd}"],
        pass_fds=(fd,), capture_output=True, timeout=8, check=True,
        env={"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
    )
    require(len(result.stdout) <= 1024 * 1024 and not result.stderr, "invalid ELF tooling output")
    return result.stdout.decode("ascii", errors="strict")


def glibc_floor(report):
    start = re.search(r"^Version needs section\b", report, re.MULTILINE)
    if not start:
        return None
    names = re.findall(r"\bName: GLIBC_([0-9]+(?:\.[0-9]+)+)\b", report[start.start():])
    if not names:
        return None
    return ".".join(str(n) for n in max(tuple(int(n) for n in v.split(".")) for v in names))


def inspect(path, target):
    name, arch, triple, libc, machine = target
    stream, original = open_regular(path, MAX_ASSET)
    with stream:
        h = stream.read(64)
        require(
            len(h) == 64 and h[:7] == b"\x7fELF\x02\x01\x01"
            and int.from_bytes(h[16:18], "little") in (2, 3)
            and int.from_bytes(h[18:20], "little") == machine,
            f"{name}: unexpected ELF class, byte order, type or architecture",
        )
        require(original.st_mode & stat.S_IXUSR, f"{name}: executable bit missing")
        programs = readelf(stream, "--program-headers")
        require(re.search(r"^\s*LOAD\s+0x", programs, re.MULTILINE), f"{name}: no loadable ELF segment")
        dynamic = readelf(stream, "--dynamic")
        interpreters = re.findall(r"\[Requesting program interpreter: ([^\]]+)\]", programs)
        needed = sorted(set(re.findall(r"\(NEEDED\).*?\[([^\]]+)\]", dynamic)))
        require(not re.search(r"\((?:RPATH|RUNPATH)\)", dynamic), f"{name}: runtime search path not allowed")
        floor = None
        if libc == "musl":
            require(not interpreters and not needed, f"{name}: static musl asset expected")
        else:
            require(
                interpreters == ["/lib64/ld-linux-x86-64.so.2"] and "libc.so.6" in needed,
                f"{name}: expected x86-64 glibc loader",
            )
            floor = glibc_floor(readelf(stream, "--version-info"))
            require(floor is not None, f"{name}: could not establish direct glibc symbol floor")
        stream.seek(0)
        digest, length = hashlib.sha256(), 0
        for data in iter(lambda: stream.read(1024 * 1024), b""):
            length += len(data)
            require(length <= original.st_size, f"{name}: file grew while hashing")
            digest.update(data)
        require(
            length == original.st_size and identity(original) == identity(os.fstat(stream.fileno()))
            == identity(path.lstat()),
            f"{name}: file changed during verification",
        )
    return {
        "file": name, "executable": "eld-engine", "os": "linux", "architecture": arch,
        "declared_rust_target": triple, "declared_build_libc": libc,
        "runtime_libc": None if libc == "musl" else "glibc",
        "linkage": "static" if libc == "musl" else "dynamic",
        "min_direct_glibc": floor, "interpreter": interpreters[0] if interpreters else None,
        "needed": needed, "bytes": length, "sha256": digest.hexdigest(),
        "verification": "binary_inspected", "qualified_targets": [],
    }


def build_manifest(directory, version, commit):
    require(len(version) <= 64 and re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", version),
            "expected a version tag such as v0.3.0")
    require(re.fullmatch(r"[0-9a-f]{40}", commit), "expected a full lowercase Git commit")
    return {
        "schema_version": 1, "project": "embedded-linux-diagnostic-engine",
        "version": version, "source_commit": commit,
        "assets": [inspect(directory / target[0], target) for target in TARGETS],
    }


def canonical(data):
    return (json.dumps(data, sort_keys=True, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def verify(path, expected):
    stream, original = open_regular(path, MAX_MANIFEST)
    with stream:
        old = stream.read(MAX_MANIFEST + 1)
        require(old == expected, "manifest mismatch: metadata, bytes or digest differ")
        require(
            identity(original) == identity(os.fstat(stream.fileno())) == identity(path.lstat()),
            "manifest changed while checking it",
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("write", "verify"))
    parser.add_argument("--asset-dir", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    try:
        expected = canonical(build_manifest(args.asset_dir, args.version, args.commit))
        path = args.asset_dir / MANIFEST
        if args.mode == "write":
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o644)
            with os.fdopen(fd, "wb") as stream:
                stream.write(expected)
        else:
            verify(path, expected)
        print(f"{args.mode}: {MANIFEST}")
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f"manifest: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

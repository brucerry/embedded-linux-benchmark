#!/usr/bin/env bash
# Produces an unsigned local preflight bundle; never publishes or runs board tests.
set -euo pipefail
umask 077
if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo 'usage: build-preflight-assets.sh ABSOLUTE_OUTPUT_DIRECTORY [VERSION]' >&2
    exit 2
fi
out=$1
version=${2-}
[[ $out = /* && $out != / && ! -e $out && ! -L $out ]] || {
    echo 'Output must be a new absolute directory' >&2
    exit 2
}
[[ $# = 1 || ( $version =~ ^v[0-9]+\.[0-9]+\.[0-9]+-preflight(\.[0-9]+)?$ && ${#version} -le 64 ) ]] || {
    echo 'Only a private preflight version is accepted' >&2
    exit 2
}
project=$(cd -- "$(dirname -- "$0")/.." && pwd -P)
cd -- "$project"
commit=$(git rev-parse HEAD)
[[ $commit =~ ^[0-9a-f]{40}$ ]]
[[ -z $(git status --porcelain=v1 --untracked-files=normal) ]] || {
    echo 'Refusing dirty source' >&2
    exit 1
}
if [[ -n ${GITHUB_SHA:-} ]]; then
    [[ $commit = "$GITHUB_SHA" ]] || { echo 'Unexpected CI source' >&2; exit 1; }
fi
rustc=${RUSTC:-rustc}
[[ $("$rustc" --version) == "rustc 1.96.0 "* ]]
package_version=$(RUSTC="$rustc" cargo metadata --format-version 1 --no-deps --locked --offline |
    python3 -c 'import json, re, sys
versions = [p["version"] for p in json.load(sys.stdin)["packages"]
            if p["name"] == "embedded-linux-diagnostic-engine"]
if len(versions) != 1 or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", versions[0]):
    sys.exit("Expected one stable numeric Engine package version")
print(versions[0])')
version=${version:-v$package_version-preflight}
if [[ $version != "v$package_version-preflight" && $version != "v$package_version-preflight."* ]]; then
    echo 'Preflight version must match the Engine Cargo package version' >&2
    exit 2
fi
sysroot=$("$rustc" --print sysroot)
lld="$sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld"
[[ -x $lld && $(uname -m) = x86_64 ]]
mkdir -m 700 -- "$out"
stage="$out/assets"
mkdir -m 700 -- "$stage"
export CARGO_TARGET_DIR="$out/cargo-target" RUSTC="$rustc"
cargo build --release --locked --offline --target x86_64-unknown-linux-gnu --bin eld-engine
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$lld" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS='-C linker-flavor=ld.lld -C link-self-contained=yes' \
    cargo build --release --locked --offline --target aarch64-unknown-linux-musl --bin eld-engine
install -m 0755 "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/eld-engine" "$stage/eld-engine-linux-x86_64-gnu"
install -m 0755 "$CARGO_TARGET_DIR/aarch64-unknown-linux-musl/release/eld-engine" "$stage/eld-engine-linux-aarch64-musl"
"$stage/eld-engine-linux-x86_64-gnu" --help > "$out/x86_64-help.txt"
grep -qF 'eld-engine pcie health' "$out/x86_64-help.txt"
python3 scripts/release_manifest.py write --asset-dir "$stage" --version "$version" --commit "$commit"
python3 scripts/release_manifest.py verify --asset-dir "$stage" --version "$version" --commit "$commit"
[[ $(git rev-parse HEAD) = "$commit" && -z $(git status --porcelain=v1 --untracked-files=normal) ]]
tar --format=ustar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -cf "$out/eld-engine-preflight-assets.tar" -C "$stage" \
    eld-engine-linux-aarch64-musl eld-engine-linux-x86_64-gnu eld-engine-release-manifest.json

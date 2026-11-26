# Diagnostic Engine release assets

The executable is `eld-engine` on both targets. The two flat binary asset names are
`eld-engine-linux-aarch64-musl` (static) and `eld-engine-linux-x86_64-gnu` (dynamic).
Neither name promises support on a particular firmware or board.

On Linux, stage the two binaries with their executable bits intact. Generate and
verify the same unsigned, deterministic manifest before uploading release assets:

```sh
version=v0.3.0
commit=$(git rev-parse HEAD)
python3 scripts/release_manifest.py write --asset-dir release --version "$version" --commit "$commit"
python3 scripts/release_manifest.py verify --asset-dir release --version "$version" --commit "$commit"
```

Schema version 1 binds each asset name to a byte count and SHA-256 and records
the intended Rust target and build libc, and the inspected CPU, OS target,
linkage, interpreter, and direct library needs. The static AArch64 binary
requires no dynamic runtime libc; the x86-64 asset needs glibc.
`min_direct_glibc` is the highest directly referenced glibc symbol version, not
a guarantee that all runtime libraries, drivers, or permissions are available.

The verifier compares canonical bytes and recomputes the binary facts. It does
not prove who built the assets, that the assets came from the stated commit, or
which libc was used to build a static executable, or that firmware or hardware tests passed. The release pipeline must bind the
source, build evidence and asset digests using independently verified provenance.
This tool emits only `binary_inspected`, with empty `qualified_targets`. Hub must
not offer automatic deployment to a specific board on this evidence alone.

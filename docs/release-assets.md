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

## CLI contract discovery

Run `eld-engine capabilities` only after verifying the executable and its
release digest. It prints one versioned JSON record without reading sysfs,
running diagnostics, or creating a report. It identifies the compiled
package version (not necessarily a release tag), target OS and architecture,
and machine-readable contracts available in this binary. Hardware,
permissions and real board support are not checked by this command. A
missing contract means a consumer must not assume that feature is available.

## Hub deployment decision

This is a consumer contract for future integration, not a claim that Hub
can deploy the Engine today. Present check, deploy and run as separate steps.

| Step | Required evidence | Stop rather than guessing |
| --- | --- | --- |
| **Check** | On the Hub host, verify access to the configured release, then check that the manifest and exact asset are available. On the authenticated Linux device, determine CPU architecture, required runtime libraries and writable executable space. | Distinguish unavailable host networking, missing release or asset, unverified provenance, and incompatible target. Never select an asset using the Hub host's CPU or libc. |
| **Deploy** | Require approved target and asset, independently verified release identity and digest, and sufficient space in a private staging directory. Transfer via the authenticated SSH session (SFTP or SCP if supported), then verify the device-side digest of the copied bytes. | A filename match, source-only release, CI preflight archive, failed transfer, or missing verified digest is not a deployment. Do not fall back to another version or serial transfer silently. |
| **Run** | After checking the copied binary, query `capabilities`, show supported commands, and run a fixed argument list only for a selected diagnostic. Save the exact asset/session identity beside the evidence. | A successful copy or capability probe does not establish device health. Separate unavailable readings, failed diagnostics, and unsupported operations. |

For a static ARM64 asset, a compatible ARM64 Linux userspace is a candidate,
not proof the board supports the kernel interfaces needed by a diagnostic.
For x86-64, require a compatible glibc loader and direct dependencies; the
recorded symbol floor is necessary but not sufficient. An empty
`qualified_targets` field means the manifest declares no board qualification.
Display that state even when a deliberately approved manual smoke test is
possible. Never flash firmware, replace a system executable, or launch stress
as a side effect of checking or transferring an asset.

For a local metadata comparison only, use the reported board facts:

```sh
python3 scripts/plan_engine_asset.py --manifest eld-engine-release-manifest.json \
  --target-os linux --target-arch aarch64
```

An x86-64 comparison also requires the observed `--glibc`, `--loader`, and
a repeated `--library` for each confirmed direct dependency. The preview
does not discover the device or check the binary contents, release origin,
or eligibility to deploy; even an unsigned preflight may yield
`metadata_candidate`. Do not treat that result as authorization.

See [passive evidence handoff](hub-evidence.md) for the run and report checks.

## Internal build preflight

On a clean Linux x86-64 checkout with Rust 1.96.0 and its
`aarch64-unknown-linux-musl` target installed, run:

```sh
RUSTUP_TOOLCHAIN=1.96.0 bash scripts/build-preflight-assets.sh /path/to/new-directory
```

The tar preserves executable modes. The CI job runs only on pushes to an
`engine/*` branch after the normal CI checks pass, and retains the tar as an
unsigned test artifact. It does not publish a release or qualify a board. The manifest label defaults to `v<Cargo package version>-preflight` in both local and hosted builds. An explicit numbered preflight label must use the same package version. This label is not a public release tag.

Before inspecting the contents of a retained private preflight tar, verify
the exact archive digest, intended preflight version, and source commit
from an independently known selection:

```sh
python3 -B scripts/check_preflight_archive.py --archive engine-preflight.tar \
  --expected-sha256 "$EXPECTED_SHA256" --version "$EXPECTED_VERSION" --commit "$EXPECTED_COMMIT"
```

The checker does not extract or execute any member. Matching archive and
member bytes is not signature verification, deployment approval, or a
hardware compatibility result.

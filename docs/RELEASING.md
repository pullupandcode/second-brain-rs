# Executable releases

Starting with v0.5.2, merging a version-bumped PR into `main` triggers the `ci`
workflow to publish native executables. Version-bumped PR runs perform the same builds
and smoke tests without a release token or publication. All existing Rust tests,
formatting, Clippy, dependency/license and vulnerability checks must pass first.
No manual tag or release creation is needed after a successful versioned merge.

## Downloads

| Platform | Target / runner | Archive |
|---|---|---|
| Linux x86-64 | `x86_64-unknown-linux-gnu`, Ubuntu 24.04 | `.tar.gz` |
| macOS Apple Silicon | `aarch64-apple-darwin`, macOS 15 | `.tar.gz` |
| macOS Intel | `x86_64-apple-darwin`, macOS 15 Intel | `.tar.gz` |
| Windows x86-64 | `x86_64-pc-windows-msvc`, Windows Server 2022 | `.zip` |

Names follow `second-brain-rs-vVERSION-TARGET.EXT`. Each archive contains the
executable, README, MIT/Apache licenses and `config.example.toml`. It never includes
local configuration, vault contents or credentials. Linux builds target the runner's
GNU runtime (Ubuntu 24.04/glibc 2.39 or compatible); older glibc and musl systems
should build from source. macOS 15 and Windows Server 2022 are the tested baselines.
macOS binaries are not Developer-ID signed or notarized. No cross-platform runtime
compatibility beyond the tested targets is claimed.

Download `SHA256SUMS` alongside the archives. On Linux run `sha256sum -c SHA256SUMS`
after downloading all listed files; on macOS use `shasum -a 256 -c SHA256SUMS`.
For a single downloaded archive compare its SHA-256 with the matching line.
PowerShell provides `Get-FileHash <archive.zip> -Algorithm SHA256`.
Extract, copy `config.example.toml` to `config.local.toml`, edit the vault/state/auth
settings, and run the executable with `--config config.local.toml`.

## Version and publication rules

1. Increase stable `X.Y.Z` in Cargo.toml and the root Cargo.lock package together.
   Add a nonempty versioned changelog section. Downgrades, mismatched metadata and
   prerelease/build-metadata versions fail validation. An unchanged version skips
   publishing; documentation-only merges do not require a bump.
2. PR CI builds all four optimized native executables with `--locked`, packages
   them and starts each extracted binary against a temporary vault. `/healthz`
   must return success. This tests the actual downloadable bytes, not only a
   debug build or archive manifest.
3. After merge, the same workflow checks the exact main commit, waits for every
   test/build job, and grants `contents: write` only to the publishing job. It
   downloads artifacts from this workflow run, verifies the complete platform set,
   computes SHA256SUMS, and creates an annotated version tag at the tested commit.
4. It creates a draft, uploads all archives and checksums, verifies uploaded hashes,
   then publishes and checks `immutable: true`. Existing tags must resolve to the
   same commit; no force-tagging, asset clobbering or deletion is implemented.

The default `GITHUB_TOKEN` is sufficient; no personal access token is required.
The repository's **immutable releases setting must remain enabled**. It was
verified enabled for this repository, and v0.5.1 provides a known immutable
baseline. That old release is a sanity check, not proof that the setting remains
on. GitHub's settings endpoint requires Administration:read, which the default
Actions token cannot obtain. The job therefore verifies the new release after
publication. If an administrator disables the setting, verification fails and
reports the mutable release without deleting, replacing or retagging it. Restoring
the setting does not retroactively repair such a release; investigate before
publishing a new version. The automation never changes repository protections.

## Retry and recovery

Publication is serialized and never cancels an active publisher. GitHub may replace
an older pending concurrency job when another is queued; inspect cancelled runs.
A stale run whose commit is no longer main fails before remote mutations (and is
rechecked before publishing). Release the next current version rather than moving
an old tag or publishing untested bytes. Use one release PR at a time.

On transient upload/API failure, rerun the failed publishing job while main still
matches its commit. It reuses only a matching tag and draft, uploads missing files,
and refuses conflicting bytes or unexpected assets. A completed immutable release
is verified read-only on rerun. Rerun the original job with its original artifacts;
rebuilding can change archive bytes, so different bytes cannot replace a prior
partial upload. Build artifacts are retained for 14 days. If they expire or a draft
conflicts, investigate manually; never rewrite a published release.

## Verification references

- [GitHub draft-first immutable release guidance](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository)
- [Release immutability and attestations](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases)
- [Repository immutability endpoint permissions](https://docs.github.com/en/rest/repos/repos#check-if-immutable-releases-are-enabled-for-a-repository)
- [Official runner platform inventory](https://github.com/actions/runner-images)

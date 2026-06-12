# Versioning

`second-brain-rs` follows [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html).

## Tags and releases

- Releases are annotated git tags of the form `vMAJOR.MINOR.PATCH`
  (e.g. `v0.1.0`), optionally with a `-rc.N` pre-release identifier or
  `+meta` build metadata.
- Each release is cut from its tag and published as a GitHub Release.
- `Cargo.toml`'s `version` always reflects the next planned release.

## Pre-1.0 policy

While the major version is `0`, the public API is considered unstable and
breaking changes may land in `MINOR` bumps, as permitted by SemVer.

Implementation phases map to minor versions:

| Phase | Release | Scope |
|---|---|---|
| 1 | `v0.1.0` | Skeleton & guardrails: config, scopes, discovery, tool registry, scope-filtered `tools/list` |
| 2 | `v0.2.0` | Read path & index |
| 3 | `v0.3.0` | Write path & framework records |
| 4 | `v0.4.0` | Schema management & OCR |
| 5 | `v0.5.0` | Auth hardening & docs |

`v1.0.0` marks the first stable, publicly-published crate release.

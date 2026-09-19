# Versioning

Use Semantic Versioning. Before 1.0, intentional public API breaks require a minor
bump; separate shipped fixes require patch bumps. Update Cargo.toml, Cargo.lock
and CHANGELOG together in each implementation PR. Documentation-only planning
requires no bump.

| Scope | Release | Delivery |
|---|---|---|
| Initial | 0.1.0 | Server skeleton and guardrails |
| A | 0.2.0 | Read/index/parser, HTTP protocol, schemas/scopes, skills/prompts, OCR contract |
| B | Planned 0.3.0; included in 0.5.0 | Atomic audited writes, deletion and recovery |
| C | Planned 0.4.0; included in 0.5.0 | Framework schemas/records, captures, daily notes |
| D (including B/C) | 0.5.0 | Production JWT/JWKS, auth hardening, final parity and deployment docs |

PR #52 was approved and merged first on 2026-09-18 at `8b98349`, bringing B, C
and D into main together. PR #50 is superseded by that merge. PR #49 merged at
`bf78768` and is published as immutable v0.5.1. Version 0.5.2 adds automatic
executable releases. Do not create
retroactive 0.3.0/0.4.0 releases or downgrade the crate.

Historical tracker phases, including skill Phase 6, are consolidated into these
four release scopes; [PARITY_PLAN.md](PARITY_PLAN.md) defines issue ownership.

Each PR requires separate code-review and QA approvals for the exact final head
and green CI. A release PR updates Cargo.toml, the root package entry in Cargo.lock,
and a nonempty `## [X.Y.Z] - YYYY-MM-DD` changelog section. Stable versions must
increase; prereleases/build metadata and downgrades are rejected by automation.
A merge with an unchanged version runs checks but does not publish another release.

From v0.5.2, the `ci` workflow builds and smoke-tests four native executable
archives on release PRs. After a version-bumped PR merges into main, publication
waits for all checks and builds, creates an annotated tag at that exact merge,
uploads every archive and SHA256SUMS to a draft, and publishes the complete release.
GitHub must report `immutable: true` before the job succeeds. PR runs cannot publish.
See [RELEASING.md](RELEASING.md) for permissions, platform baselines, retry behavior
and the repository-level immutability prerequisite. Never rewrite published tags,
replace immutable assets, or retroactively publish the skipped 0.3.0/0.4.0 stages.

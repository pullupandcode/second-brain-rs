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
and D into main together. PR #50 is superseded by that merge. PR #49 now contains
only subsequent storage review fixes and the 0.5.1 patch bump. Do not create
retroactive 0.3.0/0.4.0 releases or downgrade the crate.

Historical tracker phases, including skill Phase 6, are consolidated into these
four release scopes; [PARITY_PLAN.md](PARITY_PLAN.md) defines issue ownership.

Each PR requires separate code-review and QA approvals for the exact final head
and green CI. The original A/B/C/D sequence was superseded by the approved combined #52 merge.
Squash the remaining #49 patch onto that actual main commit. After main checks pass, create an
annotated `vX.Y.Z` tag on the squash commit. With repository immutable releases
enabled, create a draft release, finish notes/assets, publish, and verify
`immutable: true` through GitHub's API. Never rewrite published tags or releases.
If immutability cannot be verified, stop publication. Existing v0.1.0 remains intact.

# Versioning

Use Semantic Versioning. Before 1.0, intentional public API breaks require a minor
bump; separate shipped fixes require patch bumps. Update Cargo.toml, Cargo.lock
and CHANGELOG together in each implementation PR. Documentation-only planning
requires no bump.

| Scope | Release | Delivery |
|---|---|---|
| Initial | 0.1.0 | Server skeleton and guardrails |
| A | 0.2.0 | Read/index/parser, HTTP protocol, schemas/scopes, skills/prompts, OCR contract |
| B | 0.3.0 | Atomic audited writes, deletion and recovery |
| C | 0.4.0 | Framework schemas/records, captures, daily notes |
| D | 0.5.0 | Production JWT/JWKS, auth hardening, final parity and deployment docs |

Historical tracker phases, including skill Phase 6, are consolidated into these
four release scopes; [PARITY_PLAN.md](PARITY_PLAN.md) defines issue ownership.

Each PR requires separate code-review and QA approvals for the exact final head
and green CI. Squash merge in A/B/C/D order. After main checks pass, create an
annotated `vX.Y.Z` tag on the squash commit. With repository immutable releases
enabled, create a draft release, finish notes/assets, publish, and verify
`immutable: true` through GitHub's API. Never rewrite published tags or releases.
If immutability cannot be verified, stop publication. Existing v0.1.0 remains intact.

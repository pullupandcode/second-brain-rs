# Parity delivery ledger

Reference: second-brain-mcp v1.1.1 at
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`. Status snapshot: 2026-09-16.
See the [plan, ownership and adopted stories](PARITY_PLAN.md).

Scope A merged through [PR #51](https://github.com/pullupandcode/second-brain-rs/pull/51)
at `2b6dcf1c2a6cf0278a363691d57a9c20d51811cb`. After successful main CI,
[v0.2.0](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.2.0)
was published and verified `immutable: true` (release ID `390166401`).
The other three implementation PRs remain open and are rebased on that actual
squash commit. Implementation verification and release delivery remain separate gates.

| Scope / planned version | PR | Integrated tests | Merge / release |
|---|---|---|---|
| A: protocol, reads, skills, OCR / 0.2.0 | [#51](https://github.com/pullupandcode/second-brain-rs/pull/51) | 89 passed; squash tree equals reviewed `446e2e8` | Merged `2b6dcf1`; immutable [v0.2.0](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.2.0) |
| B: writes, deletes, audit / 0.3.0 | [#49](https://github.com/pullupandcode/second-brain-rs/pull/49) | 110 passed after squash rebase | Not merged or published |
| C: frameworks, records, daily / 0.4.0 | [#50](https://github.com/pullupandcode/second-brain-rs/pull/50) | 146 passed after squash rebase | Not merged or published |
| D: JWT, deployment, closure / 0.5.0 | [#52](https://github.com/pullupandcode/second-brain-rs/pull/52) | 182 passed after squash rebase | Not merged or published |

Each candidate passes nightly formatting, current-stable Clippy with warnings
denied, all-feature tests, cargo-deny and cargo-audit. Counts include inherited
scope tests and must not be summed. Code and QA reviewers independently record
APPROVE or REQUEST CHANGES against full commit SHAs in the linked PR comments;
both approvals and green CI are required on the latest head. Documentation-only
changes also receive an exact-head review. Agent reviews share the author's
GitHub identity and do not represent separate human accounts.

## Independent evidence

- Pinned TypeScript baseline: 181 passing tests across 21 files.
- All 34 tool schemas, all eight scopes, prompt access, privacy reload and real
  HTTP mutation behavior are exercised against the reference contract.
- The independent numeric comparison checks 1,068 scalar/default cases, 521
  finite writer bit roundtrips, 60 numeric-ID method cases, string IDs and actual
  HTTP frontmatter writes. Decimal parsing, unsafe integers and fractional/large
  JSON-RPC IDs now match JavaScript Number behavior.
- A 240-case method/parameter matrix verifies initialization defaults, ignored
  list/prompt arguments, metadata handling and notification behavior. Protocol
  initialization and other protocol-only preflight replies are unauthenticated,
  matching the reference; tool and prompt dispatch enforce current-request auth.
- Signed JWT probes cover 20 header/signature cases, 11 claim/scope cases, all
  eight scope listings and cross-scope denials. Independent cache tests cover
  concurrent fetching, cooldown, stale-key rejection, failed refresh and rotation.
- Fresh focused mutation testing completed 128 cases: **111 caught, 17 unviable,
  zero missed or timed out**. The final transport integration leaves all tested
  auth/configuration/argument source and auth test files byte-identical to that
  run's `e8743e2` baseline; the complete integrated unmutated suite passes.
- Forced-patch semver diagnostics identify documented intentional pre-1.0 minor
  source changes in A/B/D; C requires no public API change. D's asynchronous
  `Authenticator` remains externally implementable, independently demonstrated
  by a downstream crate despite the tool's `trait_newly_sealed` classification.

## Remaining merge and release gates

GitHub ruleset `17368994` requires one formal approval from another account.
This remains a gate for the unmerged PRs; the user completed scope A's merge.
The coordinator has not changed or bypassed repository protections. The latest
review decision and CI state are visible on each PR.

A is merged and released. Continue B → C → D. B now directly descends from A's
actual squash commit; C and D descend through the updated dependency branches.
The rebase changed ancestry without changing implementation files. Documentation
records the new merge/release state, and both independent reviewers renew their
exact-head decisions after those edits. After each subsequent squash merge, rebase the dependent PR onto the
actual squash commit, renew both independent exact-head approvals, and satisfy
CI and required GitHub review again. Each version bump is then published at the
approved merge commit after main CI passes. Native release immutability is enabled;
publish each draft only after its notes/assets are complete and verify
`immutable: true`. Never retag or replace published versions.

Scope A's adopted stories are fulfilled by #51 and v0.2.0. Remaining stories
stay open until their reviewed implementation merges; shared issue #30 waits
for both B and C. Versions 0.3.0–0.5.0 have not been published, so the full
delivery plan remains incomplete.

Reference limitations and deliberate differences are documented in [A](parity/A.md),
[B](parity/B.md), [C](parity/C.md) and [D](parity/auth.md). OCR is the reference
queued-job contract, not an OCR engine. Privacy hardening, required JWT expiry,
YAML mapping order and advertised ISO date boundaries are explicit rather than
unqualified claims of byte-for-byte equivalence.

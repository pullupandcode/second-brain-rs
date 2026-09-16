# Parity delivery ledger

Reference: second-brain-mcp v1.1.1 at
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`. Status snapshot: 2026-09-16.
See the [plan, ownership and adopted stories](PARITY_PLAN.md).

All planned handlers are implemented in four open PRs. No parity implementation
has been merged or released. The existing immutable [v0.1.0](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.1.0)
remains unchanged. Implementation verification and release delivery are separate gates.

| Scope / planned version | PR | Integrated tests | Merge / release |
|---|---|---|---|
| A: protocol, reads, skills, OCR / 0.2.0 | [#51](https://github.com/pullupandcode/second-brain-rs/pull/51) | 89 passed at `446e2e8` | Not merged or published |
| B: writes, deletes, audit / 0.3.0 | [#49](https://github.com/pullupandcode/second-brain-rs/pull/49) | 110 passed at `808ec56` | Not merged or published |
| C: frameworks, records, daily / 0.4.0 | [#50](https://github.com/pullupandcode/second-brain-rs/pull/50) | 146 passed at `cb0fbd9` | Not merged or published |
| D: JWT, deployment, closure / 0.5.0 | [#52](https://github.com/pullupandcode/second-brain-rs/pull/52) | 182 passed at code candidate `1f9bc21` | Not merged or published |

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
The coordinator's squash-merge attempt was rejected by this rule, and no
protection was bypassed. Formal review must be supplied before merging can begin.
The latest review decision and CI state are visible on each PR.

Merge A → B → C → D. After each squash merge, rebase the dependent PR onto the
actual squash commit, renew both independent exact-head approvals, and satisfy
CI and required GitHub review again. Each version bump is then published at the
approved merge commit after main CI passes. Native release immutability is enabled;
publish each draft only after its notes/assets are complete and verify
`immutable: true`. Never retag or replace published versions.

The existing issue stories remain open until their reviewed implementation
merges; shared issue #30 waits for both B and C. Merge SHAs and release URLs cannot
be recorded yet. Thus the full delivery plan remains incomplete even though its
implementation and verification work is reviewable.

Reference limitations and deliberate differences are documented in [A](parity/A.md),
[B](parity/B.md), [C](parity/C.md) and [D](parity/auth.md). OCR is the reference
queued-job contract, not an OCR engine. Privacy hardening, required JWT expiry,
YAML mapping order and advertised ISO date boundaries are explicit rather than
unqualified claims of byte-for-byte equivalence.

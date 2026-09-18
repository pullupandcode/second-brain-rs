# Parity delivery ledger

Reference: second-brain-mcp v1.1.1 at
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`. Status snapshot: 2026-09-18.
See the [plan, ownership and adopted stories](PARITY_PLAN.md).

Scope A merged through [PR #51](https://github.com/pullupandcode/second-brain-rs/pull/51)
at `2b6dcf1c2a6cf0278a363691d57a9c20d51811cb`. After successful main CI,
[v0.2.0](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.2.0)
was published and verified `immutable: true` (release ID `390166401`).
The user approved and merged [PR #52](https://github.com/pullupandcode/second-brain-rs/pull/52)
ahead of the remaining stack at `8b98349f1126c8ea02f7b4ac2c0074ae69d9acda`.
That squash has the exact tree of independently approved D head `5d5c046`,
which includes the approved B and C commits. Storage and frameworks are therefore
already on main. #50 is superseded; #49 retains only the later storage review
fixes and a 0.5.1 patch bump. Planned versions 0.3.0/0.4.0 were never published.

| Scope / delivered version | PR | Verification | Merge / release |
|---|---|---|---|
| A: protocol, reads, skills, OCR / 0.2.0 | #51 | 89 tests; reviewed squash tree | Merged `2b6dcf1`; immutable v0.2.0 |
| B/C/D: storage, framework, auth / 0.5.0 | #52 (includes #50 and original #49 scope) | 182 tests; both reviewers approved `5d5c046`; squash tree identical | Merged `8b98349`; immutable [v0.5.0](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.5.0), release ID `391790243` |
| Storage review fixes / 0.5.1 | #49 | 196 combined tests; format, strict Clippy, dependency gates and patch semver check pass; exact-head reviews/CI required | Unmerged; unreleased |

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
The coordinator has not changed or bypassed repository protections. The user
approved the combined #52 merge; #49 still requires both independent exact-head
agent decisions, green CI and the repository's formal review gate.

The original B → C → D sequence is superseded. Main CI run `35395371018`
passed for the #52 squash; v0.5.0 was published and verified `immutable: true`.
Merge #49 as the 0.5.1 patch only after its renewed gates pass, then
publish at its actual squash following main CI. Do not merge #50 again, create
retroactive 0.3.0/0.4.0 releases, or rewrite published versions.

Original B/C implementation stories were incorporated by #52; #30's shared
implementation is now present. Storage review fixes remain pending #49. Tracker
acceptance and release closure must reflect this actual delivery sequence.
The [review disposition](parity/B_REVIEW.md) documents fixes and remaining
operational limitations, including startup quarantine and interrupted soft delete.

Reference limitations and deliberate differences are documented in [A](parity/A.md),
[B](parity/B.md), [C](parity/C.md) and [D](parity/auth.md). OCR is the reference
queued-job contract, not an OCR engine. Privacy hardening, required JWT expiry,
YAML mapping order and advertised ISO date boundaries are explicit rather than
unqualified claims of byte-for-byte equivalence.

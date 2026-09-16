# Full parity delivery plan

## Target and baseline

Match the observable behavior of local `second-brain-mcp` v1.1.1, commit
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`. The TypeScript implementation and its
tests are the executable reference; README claims alone are not acceptance evidence.
Target all 31 base tools, three optional OCR job-contract tools, eight scopes,
MCP prompts, HTTP transport, configuration, filesystem policies, and deployment.
OCR parity means the reference job contract, not a new OCR engine.

Starting Rust commit: `f50f4286d95ec6200431d88ebee3b6b895fc3ad6` on `phase-2`.
Remote main is the released v0.1.0 skeleton. The existing uncommitted
`tests/integration.rs` changes belong to the user and must be preserved; copy them
into scope A's isolated worktree. Initial validation: 51 unit tests pass; 11/12
integration tests pass outside the network sandbox. Empty-query search fails.

## Tasks and ownership

| ID / owner | Release | Scope and acceptance | Dependencies |
|---|---|---|---|
| A / protocol agent | 0.2.0 | Complete read/index/parser behavior; root folder and empty search inputs; exact tool schemas and scope registry including delete and skills tools; MCP initialize/list/call, per-request auth propagation, errors and structured results; skill maps/loading/reloading/prompts and effective privacy policy; optional OCR queue/status/renumber contract; read/transport/skills/OCR tests; baseline docs | Existing phase-2 work |
| B / storage agent | 0.3.0 | Atomic create/replace/frontmatter/marker writes, optimistic concurrency, per-path serialization, cooldown, traversal/symlink/conflict protection, soft/hard deletion, audit lifecycle/rotation/recovery; runtime wiring and write/delete config; negative and failure-path tests | A integrated before merge |
| C / framework agent | 0.4.0 | YAML schema validation, LYT/PARA/Zettel presets, overlays and registry persistence, initialize/reload/register/unregister/list/compose; record types/maps/structure, templates, records, idempotent source captures, daily get/append/repair and both capture strategies; route every mutation through audited writer | B writer APIs; A privacy policy |
| D / auth agent | 0.5.0 | Actual RS256/ES256 JWT/JWKS verification, issuer/audience/time/algorithm/subject checks, caching and rollover, fail-closed auth, dev loopback restriction, HTTP challenges/discovery and eight scopes; comprehensive auth tests; final configuration/user/deployment docs and parity closure | A protocol; final rebase after C |

Each owner owns its new modules and tests. Shared files (`Cargo.toml`, lockfile,
config, runtime, module exports, registry, MCP/HTTP) are edited in separate
worktrees and reconciled explicitly at integration. No agent edits another
agent's worktree. Storage exposes reusable audited mutation methods so framework
code cannot bypass write policy; agents coordinate the concrete API before wiring.
Skills privacy policy must apply to every read, index query, write and framework
mutation, including after reload. Auth changes may change the auth trait to async.

## Test-driven execution

1. Inventory the reference implementation and tests for the owned scope.
2. Add behavior tests and record a failing run before implementation (RED).
3. Implement the smallest complete behavior and record passing tests (GREEN).
4. Include success, invalid input, scope denial, blocked paths, concurrency and
   relevant persistence/failure paths. Exercise actual MCP HTTP calls, not only
   internal dispatch. Do not remove failing coverage to get green.
5. Record reference mapping, commands/results, and deviations in a scope report
   under `docs/parity/`. A intentional security improvement must be explicit;
   an unimplemented behavior cannot be called parity.
6. Run required checks: nightly format check, clippy all targets/features with
   warnings denied, all-feature tests, cargo-deny and cargo-audit. Run semver-checks
   for changed public APIs and classify intentional pre-1.0 minor breaks.

## PR, review and merge gates

Four independent implementation agents each open a PR. Execution uses waves
because this session supports the coordinator plus three concurrent agents.
Two separate verification agents, neither the author of the PR, inspect each
candidate: one code reviewer and one QA reviewer. They do not rely on each
other's conclusions. Review the final integrated candidate, not an obsolete
pre-rebase version.

- Code review: inspect the complete diff, reference behavior, security, async
  and filesystem safety, public API, maintainability and version/changelog.
- QA: independently run checks and adversarial/end-to-end cases, inspect the
  full parity checklist, and confirm RED/GREEN evidence and release metadata.
- Both report explicit APPROVE or REQUEST CHANGES with the exact head SHA.
  New commits invalidate both approvals. Fixes are rechecked by both agents.
- Publish review findings on the PR. Agents share one GitHub identity; if GitHub
  forbids self-approval, post clearly labeled independent-agent review comments
  rather than representing them as distinct GitHub human approvals.
- CI must pass on that same head. Squash merge in A, B, C, D order only after
  both approvals. Rebase dependent work onto actual squash commits, removing
  already-integrated baseline changes. Never bypass branch protections.

## Version and immutable release gates

Each completed scope is one release item and receives the minor version above.
Any separate shipped bug fix receives a patch bump. Documentation-only planning
does not bump the crate. Update Cargo.toml, Cargo.lock and CHANGELOG together
inside the reviewed PR. No speculative bump is published before completion.

Enable GitHub release immutability before publishing new releases. After an
approved squash merge and passing main CI, create an annotated `vX.Y.Z` tag at
the merge commit, push it, create a draft GitHub release, finish notes/assets,
then publish and verify `immutable: true` through GitHub's API. Do not rewrite
or delete published tags/releases. Do not modify the existing v0.1.0 release.
If immutability cannot be enabled or verified, stop publication and report the
specific blocker instead of publishing a mutable release.

GitHub references:
[immutability settings](https://docs.github.com/en/code-security/how-tos/secure-your-supply-chain/establish-provenance-and-integrity/prevent-release-changes),
[repository REST API](https://docs.github.com/en/rest/repos/repos).

## Completion criteria

All four PRs have both independent approvals, green required checks, squash
merges, matching version metadata and verified immutable releases. Every target
tool has a functioning handler; no successful `not_implemented` response remains.
All eight scopes and prompt operations are exercised over HTTP. The combined
suite passes; no secrets or blocked data leak; production JWT mode rejects
development tokens. Documentation states actual behavior and known reference
limitations. The coordinator records PR URLs, merge SHAs, review SHAs, test
results and release URLs in the delivery ledger.

## Adopted repository stories

The existing tracker is authoritative for task details, with behavior checked
against the pinned reference. No duplicate story set will be created.

| Owner | Stories |
|---|---|
| A | #19, #33, #43–#48; audit previously closed #12–#18 |
| B | #20–#25, storage portion of #30, #40 |
| C | #26–#29, framework portion of #30, #31–#32, #34–#36 |
| D | #37–#39, #41–#42 |

#30 is shared and closes only after both B and C acceptance passes. Existing
phase labels are historical; release prep in #36 lands in 0.4.0, #42 in 0.5.0,
and skill release prep #48 is pulled forward into A's 0.2.0. Correct #46's stale
33-base-tool claim to 31 base / 34 with OCR. All eight scopes are required at
completion. Issue #41 adds focused mutation testing for auth and validation.
Report completion on issues only after the corresponding reviewed PR merges.
GitHub immutability was confirmed enabled before work began.

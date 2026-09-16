# Scope A: read, protocol, skills and OCR parity

Reference: second-brain-mcp v1.1.1, commit
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`. Adopted #19, #33, #43–#48;
rechecked the prior #12–#18 read foundation. This PR incorporates the existing
phase-2 baseline and preserves the user integration tests copied into this worktree.

## Reference mapping

| Reference | Rust / evidence |
|---|---|
| auth/scopes.ts, tools/registry.ts, server.ts schemas | Eight scopes; 31 base/34 OCR tools; fixture test compares every schema |
| server.ts MCP routes | HTTP initialization, tool listing/calls, current-request authorization, errors, structuredContent, prompts, parsing limits/header binding |
| skills/loader.ts | skills.rs map links/hints, name/description/body validation, dedup/sort, safe diagnostics |
| runtime.ts effectiveBlockedPaths / reload | Shared PathPolicy covers configured maps, all successful/failed candidates and canonical targets; swap on reload |
| ocr/jobs.ts | UUID queued notebook/renumber jobs, optional arguments, timestamps, status/missing discriminator |
| vault/reader.ts, index.ts, markdown.ts | Tempfile reads/search/list/links/conflicts, path security, parser and escaping properties |

Schema fixture `tests/fixtures/v1.1.1-tool-schemas.json` was extracted from the
reference build's pure schema functions and tool names; it covers all 34 schemas.

## Test-driven evidence

- RED: initial `cargo test --test integration -- --nocapture` outside the listener
  sandbox: 10 passed / 6 failed. Failures: scope count, empty query, missing HTTP
  authentication, initialization/prompts, skills/OCR runtime contract.
- GREEN: coordinator rerun after wiring: 16/16 integration tests passed over real
  localhost HTTP, including scope changes between requests and skill reload/OCR.
- RED: escaped scalar/list roundtrip failed (literal backslash escape retained).
  GREEN after JSON string/list decoding; full library suite 54/54 passed.
- RED: canonical skill/map aliases exposed Notes/skill.md; GREEN after adding
  resolved targets to effective policy, also checked through writer policy handle.
- RED: preflight oversized unauthenticated body returned401 instead of413;
  malformed/header-binding cases now pass with bounded preauthentication parsing.
- RED: list_folder(file.md) returned success; now propagates sanitized read errors.
- QA RED: initialize leaked rmcp build identity and default protocol version;
  explicit package identity and requested protocol version now have HTTP regressions.
- RED: a mode-000 Private/** directory caused conflict-scan startup failure;
  GREEN after applying the effective policy before stat/open of any subtree.
- RED: malformed search tag/folder values were silently ignored and broadened
  results; GREEN after explicit object/string validation (an intentional stricter
  input contract than the reference unchecked filter properties).
- RED: OCR rejected equivalent integer spellings such as 1.0 and 2e0; GREEN
  after validating numeric integrality rather than the JSON lexical representation.
  Negative page integers remain accepted as in the reference; fractions and
  nonnumeric values are rejected. The reference defines no page min/max bounds.
- Security dependencies checked centrally: cargo-deny/audit pass without waivers.
  Final validation commands and candidate head are recorded on the PR.

## Deliberate improvements and limits

- Empty search queries and root folder paths are accepted per documented schemas
  and index/reader contracts, fixing the reference runtime's nonempty-string bug.
- CRLF frontmatter normalization is retained from the Rust baseline. JSON-escaped
  scalar and string-array decoding makes audited writer serialization roundtrip;
  the reference parser's literal-escape/comma splitting bug is not reproduced.
- Tags now follow reference frontmatter-first ordering; outgoing links come from
  the body, correcting baseline tests that previously included frontmatter links.
- Privacy includes canonical targets of maps/skills and alias reads. Missing,
  invalid, ignored candidates remain protected. Unlike the reference, malformed
  map links produce safe diagnostics instead of aborting server startup/reload.
- JWT mode rejects every token until D supplies a production verifier. A requires
  credentials even for initialize; D owns final auth behavior. Development tokens
  remain restricted to configured loopback listeners.
- No OCR engine is claimed: jobs stay queued and are memory-only, as in reference.
- No background filesystem watcher exists in the pinned runtime. The startup
  index remains stale after external edits, including skills unloaded by reload.
- Write/framework/daily/capture handlers remain explicit tool errors pending B/C.
  Full parity and production readiness are not claimed in this release.

## Public API compatibility

A forced patch-level `cargo semver-checks` comparison against v0.1.0 identifies
three intentional pre-1.0 minor-release changes: `SecondBrainHandler::new` now
requires a runtime argument; the shared runtime means the handler no longer
implements `UnwindSafe`/`RefUnwindSafe`; and inserting the three added scopes
changes Rust enum discriminants. OAuth wire strings are stable. These changes
are released as 0.2.0, never as a 0.1.x patch; downstream Rust integrations must
construct a runtime and must not persist numeric Scope discriminants.


## Configured hard-deny case aliases (review regression)

The reviewer reproduced `Private/**` failing to protect an actual lowercase
`private` directory on a case-insensitive filesystem. Canonicalizing existing
paths alone cannot protect differently cased configured patterns or not-yet-created
prefixes. PathPolicy now NFC-normalizes hard-deny patterns and candidate paths before
anchored Unicode case-insensitive matching, using unicode-normalization0.1.25
and the existing regex dependency. It retains the
same supported glob subset and original spelling in snapshots. New rules compile
before atomic replacement so every shared reader, index query and future writer
sees the same policy after skill reload. A regex compilation failure fails closed.
The generic case-sensitive matcher used by soft ignored globs is unchanged.

This is intentional stricter security behavior than the pinned reference: even
on a case-sensitive filesystem, a deny also protects differently cased distinct
paths. Ordinary path identity, write-lock identity and index identity are not
case-normalized. Unicode simple case folding includes variants such as sigma;
normalization applies only to hard-policy comparisons, not ordinary path identity.

RED on predecessor `ffe4a0ac7bd93a7f30d335787e70209b5fb71b4e` with only new tests:
`hard_denies_ignore_case_in_existing_and_future_paths` missed private/secret.md;
`hard_denies_cover_configured_case_aliases_and_skill_reload` missed a future path.
Both tests passed after implementation. The integration test exercises configured
case-mismatched read/list/search denial, shared future-write-path policy, and
reloading a previously indexed public skill into private prompt state.
Raw receipts: `/private/tmp/parity-admin/a-case-policy-red.log`,
`a-case-integration-red.log`, `a-case-policy-green.log`, and
`a-case-integration-green.log` (same directory). Final full-suite receipts are
maintained by the coordinator for the reviewed head.


## Canonical Unicode normalization aliases (QA regression)

Independent QA then reproduced a composed NFC `Futuré/**` rule failing to protect
an NFD `Future\u{301}` directory on a normalization-insensitive filesystem.
Hard-deny compilation and candidate matching now both normalize to NFC before the
existing Unicode casefolding. Configured pattern snapshots retain their original
spelling; ordinary reader/index/writer path identity and soft ignored globs do not
normalize. Missing prefixes and rule replacements receive identical protection.
This intentionally also denies canonically equivalent distinct filenames on
normalization-sensitive filesystems. Canonical normalization is NFC, not NFKC.

RED: `hard_denies_normalize_canonical_unicode_equivalents` missed the nonexistent
NFD prefix; `hard_denies_normalize_unicode_across_reads_and_skill_reload` exposed
the existing NFD protected note. The regressions also cover both NFC/NFD directions,
read/list/search denial before/after skill reload, newly private Unicode skill
aliases, snapshot preservation, and future-write-path policy. Raw receipts are
`/private/tmp/parity-admin/a-nfc-policy-red.log` and `a-nfc-integration-red.log`;
matching `a-nfc-policy-green.log` and `a-nfc-integration-green.log` contain passing
runs. The new dependency is unicode-normalization0.1.25, verified/fetched by the
coordinator; the final dependency audit must be rerun for this candidate.


## ECMAScript numeric scalar parity (review regression)

The pinned Markdown parser accepts numbers only when
`Number.isFinite(Number(raw)) && String(Number(raw)) === raw`. Rust's default
float display differs around decimal/exponent thresholds and negative zero.
`vault::markdown::parse_canonical_number` is now a crate-visible shared helper
using ryu-js1.0.3 for ECMAScript canonical display after an explicit finite guard;
framework schema parsing can reuse the same helper. Raw scalars that fail the
canonical spelling check remain strings, rather than being coerced to numbers.

The 30-case table was independently checked with JavaScript and includes 1e-7,
1e+21, decimal/exponent threshold neighbors, -0, signed/capitalized exponents,
minimum subnormal, maximum finite double, overflow and nonfinite values. RED on
predecessor83b83b5: 1e-7 was incorrectly a string; receipt
`/private/tmp/parity-admin/a-numeric-red.log`. GREEN after implementation is
`a-numeric-green.log` in the same directory. The coordinator verified/fetched
ryu-js1.0.3; dependency checks must run on the updated lockfile.


## Raw JSON number and request-ID parity (QA regression)

A raw HTTP JSON argument `1.9140508460772142e+26` lost one ULP in serde_json's
non-roundtrip float parser. Enable its `float_roundtrip` feature so the preflight,
rmcp arguments and response decoding all preserve the correctly rounded f64.
No new dependency is introduced. The regression sends literal JSON through the
Axum/rmcp/OCR queue/status path and compares returned bits with the known value.
RED/GREEN: `/private/tmp/parity-admin/a-json-number-{red,green}.log`.

rmcp represents numeric request IDs as i64 and otherwise silently interpreted
valid fractional/large numeric-ID requests as notifications. The stateless HTTP
adapter now substitutes a string only for numbers outside rmcp's representation
and restores the reference-rounded JSON number on that request's JSON result/error.
There is no shared mapping or session state. Ordinary integer/string IDs and
notifications retain their behavior. The regression covers positive/negative
fractional and large numbers, integral float spelling, success/error replies,
a string resembling an adapted ID, and notification202 with an empty body.
RED/GREEN: `/private/tmp/parity-admin/a-numeric-id-{red,green}.log`.

Input numbers are normalized to ECMAScript binary64 precision, including integer
literals beyond 2^53 and signed zero. Canonical integer representations are retained
for integer argument validation; numeric strings are unchanged. This normalization
also applies to IDs in early preflight results/errors. The regression covers safe
integer controls, positive/negative 2^53+1, u64::MAX, signed zero, and OCR integer
arguments. RED on the pre-normalization adapter returned 9007199254740993 instead
of 9007199254740992; GREEN rounds it like JSON.parse.
Receipts: `/private/tmp/parity-admin/a-json-integer-{red,green}.log`.

## Method parameter tolerance (independent review regression)

The reference HTTP dispatcher accepts initialize with absent, null, scalar, array,
or incomplete params: it uses a nonempty protocolVersion string or defaults to
2025-03-26, without authentication. The preflight now returns this handshake with
the Rust package identity. Tools/list and prompts/list ignore params entirely.
Before rmcp decoding, tools/call retains only the validated name and optional
object arguments, while prompts/get retains only its validated name. Unused
cursor, _meta, capability, client-info and prompt argument fields therefore do not
cause SDK deserialization errors. Header/name binding still checks the original
request first; tool argument validation and scope authorization remain in place.

Three regression tests failed on the preceding adapter (initialize missing result,
list/call HTTP415) and pass after normalization. They cover absent/null/scalar/
array/object params, protocol fallback/echo, unauthenticated initialization,
ignored metadata, numeric/string-compatible IDs and scoped lists. Raw receipts:
`/private/tmp/parity-admin/a-params-{red,green}.log`. The four numeric boundary
regressions remain green (`a-params-numeric-green.log`). Independent review's
240-case matrix covers the same method/parameter boundary without extending scope.

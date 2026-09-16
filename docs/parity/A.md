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

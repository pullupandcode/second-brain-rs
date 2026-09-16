# Storage parity scope B

Target: second-brain-mcp v1.1.1 / 48b272337a6ef7e381fd175b2ff1844c02cebd7a.
Stories: #20–#25, primitive storage portion of #30, #40. Release 0.3.0.

## Reference mapping

| Reference | Rust implementation / coverage |
|---|---|
| vault/writer.ts createNote/replaceNote | writer.rs; atomic CRUD, duplicate paths, stale hashes, cooldown, normalized-path serialization, concurrent no-clobber creates |
| updateFrontmatter, withFrontmatter | merge preserving body; scalar/list serialization, validated keys, roundtrip property test |
| replaceSectionByMarker | preserve outside text; missing/reversed marker failure without mutation |
| deleteNote/hardDeleteNote | hash/cooldown/policy guard chain; trash collision preserving both notes; permanent unlink |
| vault/audit.ts | SQLite tables, operation/event checks, append-only triggers including replacement inserts, bounded recent/incomplete queries, UUIDv4 attempt IDs |
| vault/tools.ts auditedWrite | start, succeeded/failed lifecycle and successful provenance; persistence failure remains best effort to avoid retries of completed writes |
| runtime.ts audit startup/recovery | startup rotation, durable store, list_write_recovery_diagnostics; primitive write dispatch |
| config.ts deletes | normalized configured trash destination, default `.trash/mcp` |

## TDD evidence

The first `cargo test --test storage` failed with unresolved imports for missing
`vault::writer` and `PathPolicy` before their implementation. After implementation,
the original three behavior tests passed (CRUD/hash conflicts, policy/cooldown,
frontmatter/markers/concurrency). Expanded tests cover reloadable policy, symlink
escapes, trash collisions, append-only SQL tampering, crash-shaped incomplete
attempts across reopen, rotation threshold/name and an empty existing database,
audit persistence fault injection, and concurrent creates from independent writers.
The property test covers valid frontmatter scalars and body preservation.

## Deliberate differences and reference limitations

- No unsolicited frontmatter provenance fields are inserted into generic notes.
  Story #21 mentions `mcp_write_id`/`mcp_written_at`, but neither exists in the
  pinned reference. The reference's `source_client` is capture-record metadata
  (scope C). Audit rows carry operation, hashes, UUID attempt and timestamps.
- In-vault symlink aliases are rejected for writes, preventing alias bypass of
  hard blocks, cooldown/lock identity and conflict policy. The reference accepts
  in-root aliases. External filesystem mutation races remain subject to the host
  filesystem; callers should not share a writable vault with untrusted OS users.
- Creates and trash moves atomically reserve destinations with hard links; this
  prevents overwriting concurrent creations or old trash. The temporary file is
  fsynced before replacement. Trash remains on the same filesystem as the vault.
- Conflict files themselves are quarantined, and runtime startup populates the
  canonical conflict quarantine from detected sync conflicts.
- Both audit tables protect `INSERT OR REPLACE`, extending the reference guard
  on successful-write rows to lifecycle rows.
- Runtime primitive mutations rebuild the index immediately. This makes completed
  writes searchable without waiting for a filesystem watcher. Refresh failure is
  logged and does not convert a completed mutation into a retryable failure.
- Cancellation during a filesystem mutation can leave an incomplete lifecycle
  diagnostic and a temporary file; no automatic rollback is claimed. Recovery
  diagnostics deliberately report ambiguity rather than guessing completion.

## Integrated candidate

The B commits are rebased onto approved A head
`ffe4a0ac7bd93a7f30d335787e70209b5fb71b4e` pending its squash merge. Runtime,
reader, index and writer share A's reloadable PathPolicy. Existing OCR coded
errors, MCP request authentication, prompts and exact eight-scope registry are
preserved. The three added HTTP tests exercise primitive write roundtrips,
frontmatter/marker updates, current-hash conflict data, immediate search refresh,
separate delete scopes, blocked/quarantined paths, recovery diagnostics, and
skill reload blocking every mutation. A's old pending-create assertion now checks
successful creation because storage is implemented.

Two additional runtime tests verify configured trash destinations, rotation during
Runtime::create, and invalid input errors without creating files or successful
audit rows. String/list serialization tests cover escaped quotes, backslashes,
newlines and embedded commas against A's parser.

The final rebase onto A's actual squash commit remains necessary before merging B.
Both independent reviewers must approve the resulting exact head, and required
CI must pass. C consumes Runtime::writer() for audited framework mutations.

## Standalone draft checks (2026-09-16)

- `cargo +nightly fmt --all -- --check`: passed.
- `cargo clippy --all-targets --all-features -- -D warnings`: passed.
- `cargo test --lib`: 51 passed; `cargo test --test storage`: 10 passed.
- Earlier full run passed 51 library, 5 inherited HTTP, 8 storage tests. Latest
  full run passed library tests, then all 5 HTTP tests were blocked at listener
  binding by sandbox `Operation not permitted`; coordinator rerun required.
- `cargo-audit audit --no-fetch`: passed, 276 dependencies / 1246 cached advisories;
  sandbox prevented crates.io index lock acquisition, reported as a warning.
- `cargo-deny --offline check`: coordinator escalated rerun passed all four gates.
- `cargo-semver-checks ... --baseline-rev f50f428...`: dependency refresh needs
  network; routed to coordinator. New APIs are additive; 0.3.0 is a pre-1.0 minor.

These preliminary checks do not replace final integrated-head review and QA.

## Integrated verification (2026-09-16)

- Nightly format and all-target/all-feature Clippy with warnings denied: passed.
- Storage integration tests: 11 passed; runtime mutation/retention tests: 2 passed.
- Coordinator rerun of HTTP storage and reload privacy tests: passed with socket
  access. Local sandbox cannot bind TCP listeners; central full-suite run records
  the final HTTP result.
- Dependency gates remain required on the final reviewed head; no advisory or
  license waiver was introduced.
- Central full-suite run passed: 55 library + 25 HTTP/integration + 11 storage
  tests (91 total). The two additional storage_runtime tests passed separately
  after that run. All 93 tests are included in the candidate's required CI run.

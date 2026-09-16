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

## Integration still required before approval

This draft begins on the existing phase-2 baseline. A must merge first. Rebase B
onto A's squash merge, keep A's PathPolicy implementation and share Runtime's
policy with the writer so skill reload changes apply. Preserve A's protocol error
mapping and exact deletion scopes/schema. Exercise create/update/delete/recovery
and denied scopes over HTTP against that integrated candidate. C consumes the
shared audited `Runtime::writer()` API for all framework-related mutations.
Run all required gates on the final rebased candidate and record exact results.

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

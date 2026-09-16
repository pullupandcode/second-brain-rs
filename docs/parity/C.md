# Scope C: framework parity

Target: TypeScript v1.1.1 `48b272337a6ef7e381fd175b2ff1844c02cebd7a`.
Release candidate: 0.4.0. Implementation PR: [#50](https://github.com/pullupandcode/second-brain-rs/pull/50).

## Reference and story mapping

`src/framework/{schema,presets,registry,tools,records,daily}.ts`, framework argument
adapters and map discovery in `src/runtime.ts`, and their corresponding unit tests
are the executable reference. The copyable LYT schema is sourced from the same
reference checkout and parsed by an integration test.

This scope covers #26–#29, the framework portion of #30, #31–#32, and #34–#36.
All framework, record/capture, and daily handlers are wired; vault structure
includes effective record types, and map discovery uses schema folders.
Registrations persist in priority/name order, deduplicate by name, and compose
without restart. Concurrent registry updates serialize to avoid lost entries.

Issue #26 also mentions daily-section composition and MOC-folder union rules.
The pinned schema defines neither field nor rule. Those extra story requirements
are outside the adopted executable-reference scope and must not be represented as
implemented. The reference uses a YAML subset (including inline arrays and field
declarations); this port uses the same subset without a general YAML dependency.
Leading indentation is bounded to 256 spaces to keep parser recursion bounded.

## Reference limitations and security improvements

`capture_default_pattern` is parsed but never consulted by the reference runtime.
Pattern A is the explicit daily append workflow; pattern B is explicit capture
records. Captures are not silently redirected based on the setting.

Frontmatter defaults and required/type declarations are parsed and exposed.
Matching the reference, record creation synthesizes `scheduled` and normalizes
meeting attendees; it does not enforce declarations or apply `defaultValue`.
Missing record templates are tolerated. Daily templates remain required.

Serialization difference: written YAML mapping keys use deterministic alphabetical
order rather than JavaScript insertion order. Parsed fields, note body content,
and ordered list values retain equivalent semantics; byte-for-byte mapping key
order is not claimed.

Framework schemas, registries, templates, and note workflows enforce the shared
effective privacy policy, including after skill reload and through canonical
aliases. Raw metadata reads recheck policy after filesystem awaits. The
TypeScript framework management code does not enforce this privacy boundary.

YAML/JSON metadata uses dedicated atomic publication without note cooldown.
All note mutations use the audited writer. A schema output path ending in `.md`
therefore also uses the audited writer and cooldown checks; this closes the
reference's administrative route around note auditing. Metadata writes refuse
symlink components, use exclusive temporary files, and publish complete content.

## Test-driven evidence

| Area | RED | GREEN |
|---|---|---|
| Schema, composition, presets | Missing parser/composer/preset functions, 10 E0425 errors | Four initial unit tests passed |
| Runtime workflows | Five tests returned `NotImplemented` | Record, capture, daily, management, map and negative-path tests pass |
| Date overflow | `2026-02-30` rejected | Normalizes to March 2 like JavaScript; day 32 and month 13 rejected |
| Markdown schema auditing | Expected one audit row, observed zero | `.md` schema creation audited; immediate overwrite respects cooldown |
| HTTP integration | Pre-A transport rejected tool request with 422 | Actual HTTP overlay/capture/daily/scope round-trip passes after A integration |
| Daily error codes | Generic errors lacked stable discriminators | `section_missing`, `section_not_writable`, and `markers_missing` preserved |
| Registry numbers | JSON `1.0` rejected as noninteger | Integral floats and large integer-valued numbers match reference acceptance and ordering |
| YAML inline comments | Comment after an earlier literal hash remained in the value | Whitespace-prefixed comment is removed like the reference subset |
| Metadata case aliases | `private/SCHEMA.yaml` overwrote an existing file under blocked `Private/**` | Canonical target/ancestor identity is checked; existing and new destinations under that folder are blocked |
| Existing structure test | Fixture had no schema and composition failed | Initialize schema, retain original privacy assertions, assert five record types |

Additional regressions cover source-ID replacement and immediate indexing,
optimistic hashes, UTC/timezone filename expansion, scheduled fields, attendee
links, missing/private templates, marker repair, protected sections, schema and
registry symlink escapes, concurrent registry persistence, metadata cooldown
isolation, sample YAML parsing, and invalid input without filesystem changes.

Three reload regressions protect newly private captures/daily notes/templates,
canonical schema and registry targets reached through skill aliases, and an
unlisted template alias whose canonical target becomes private. These use the
actual shared runtime policy and `skills_reload` handler.

## Validation and integration status

- `cargo +nightly fmt --all -- --check`: passed.
- `cargo +stable clippy --all-targets --all-features -- -D warnings`: passed,
  without suppressing inherited lints.
- Actual HTTP framework acceptance: passed in the coordinator's permitted local
  socket environment; the previous HTTP failure is resolved.
- `cargo +stable test --all-features`: passed all 121 tests: 62 library,
  19 framework workflows (including actual HTTP), 25 protocol/read integration,
  13 storage, and two storage HTTP tests. No tests ignored or filtered.
- `cargo deny check` and `cargo audit`: passed with refreshed advisory data.
- Forced-patch semver comparison against storage baseline `62f3df4`: passed
  223 checks (31 inapplicable checks skipped), no API break detected. The forced
  patch classification ensures pre-1.0 minor versioning does not bypass checks.

Validation logs are held by the coordinator as `c-revised-{fmt,clippy,tests}.log`
and `c-final-{deny,audit,semver}.log`. The canonical metadata regression ran on the
case-insensitive development filesystem; it detects and returns early on systems
where those alternative spellings refer to distinct files.

The tested source candidate `9f21526` integrates approved protocol behavior and
storage candidate `b64bc10`. A separate shared-policy review remains open for
newly created, differently cased blocked subtrees without an existing canonical
ancestor. Any resulting baseline fix must be inherited and rechecked before
final approval. Rebase onto the actual A/B squash commits is still required once
those merges are permitted. Independent code review and QA must approve the final SHA;
new commits invalidate earlier approvals. No 0.4.0 tag or release is published by
this branch, and no unmerged story is marked completed.

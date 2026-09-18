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

Ordered record-type names, equal-priority registry names, map paths, and structure
folder paths use ICU English (`en-US`) collation, matching the pinned Node
process's default locale. The locale is fixed for reproducibility; deployments
whose JavaScript process uses another default locale can produce a different
order. Registry priorities `-0.0` and `0.0` compare equally before the name tie-break.

Date inputs support ISO year/month/day defaults, timestamps with or without
seconds, fractional seconds truncated to milliseconds, UTC/numeric offsets,
24:00 rollover, and day overflow through day 31. Date-only inputs are UTC;
timezone-less datetimes use the server's system timezone with JavaScript-compatible
DST gap/overlap resolution. Filename/frontmatter years use the reference's
unpadded numeric year. Signed extended ISO years cover JavaScript's full
TimeClip range (inclusive ±8.64e15 milliseconds since the Unix epoch), with clipping
after timezone conversion. For local years outside the timezone library's civil
range, equivalent Gregorian 400-year cycles preserve IANA's initial historical
offset and final recurring rules, including compatible DST gap/overlap handling.
Node-derived cases cover New York, Lord Howe and Apia, distant past/future seasons,
historical offsets with seconds, and exact clipped endpoints.

The advertised argument contract in reference `src/server.ts` is ISO date/datetime.
JavaScript's implementation-dependent legacy strings (for example English or
slash-separated dates) remain intentionally unsupported; this is narrower than
unrestricted `Date.parse`, while preserving the advertised ISO contract.

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
| Locale ordering | Independent HTTP QA returned `[Alpha,Zulu,alpha,beta]` instead of `[alpha,Alpha,beta,Zulu]` | ICU ordering covers mixed case and accented registry/map/structure paths; numeric zero ties preserve name ordering |
| ISO timestamp forms | Independent QA rejected overflow timestamps, year-month and timezone-less datetimes | Shared parser accepts/normalizes these forms; local DST gap/overlap and invalid leap seconds are covered |
| Extended ISO years | `+010000-01-01T12:00:00Z` returned no parsed date | Large-date support, final TimeClip, extended serialization and local timezone displacement pass Node-derived regression cases |
| Schema numeric defaults | `1e-7` incorrectly remained a string under Rust float formatting | Shared ECMAScript canonical-number parser matches exponent thresholds, maximum finite/minimum subnormal values and signed zero; direct finite JSON values preserve numeric types, and integer version `1` remains valid |
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

The prior 121-test gate above predates the subsequent shared-policy and QA fixes.
The candidate now inherits shared case-insensitive denies from A `05c9741` through
B `a1690cc`, addressing nonexistent blocked prefixes as well as existing aliases.
After the QA and extended-year fixes, focused validation passes 13 framework library tests, 20
framework runtime tests (HTTP excluded only in the local sandbox), and strict
Clippy. Full HTTP/dependency/repository checks remain required for the new candidate.
The extended-year workflow test also passes for records, both capture tools and
daily notes. The reference QA failure log is `/private/tmp/parity-qa-c/results.log`.

Rebase onto the actual A/B squash commits is still required once those merges are
permitted. Independent code review and QA must approve the final SHA; new commits
invalidate earlier approvals. No 0.4.0 tag or release is published by this branch,
and no unmerged story is marked completed.

Numeric review follow-up: `cargo +stable test --offline --lib framework::schema::tests::`
passes all seven schema tests. The boundary table covers `1e-7` / `0.0000001`,
`0.000001`, `1e+21` / its expanded decimal spelling, `1e20`, maximum finite,
minimum normal/subnormal, zero, signed zero and overflow. Maximum finite and
smallest normal/subnormal defaults also survive serialization/deserialization of
the schema response with exact float bits. No `float_roundtrip` feature or new
dependency was needed. The shared writer numeric-serialization follow-up must be
inherited before the final combined review.

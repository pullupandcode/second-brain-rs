# Scope C: framework parity

Target: TypeScript v1.1.1 `48b272337a6ef7e381fd175b2ff1844c02cebd7a`.

Reference mapping: `src/framework/{schema,presets,registry,tools,records,daily}.ts`,
`src/runtime.ts` framework argument adapters and map discovery, corresponding
`tests/unit/framework-*.test.ts`, `daily-notes.test.ts`, and `runtime.test.ts`.

## Test-driven evidence

- RED: `cargo test framework::schema --lib` failed with missing parser/composer/
  preset functions (10 E0425 errors).
- GREEN: same command passed 4 schema/composition/preset tests.
- RED: `cargo test --test framework_parity` failed 5 workflow tests with
  `DispatchError::NotImplemented`; one negative-path test passed vacuously and
  is retained alongside successful workflow tests.

## Reference limitations and adopted story scope

Issue #26 mentions daily-section composition and MOC-folder union rules, but the
pinned schema does not define these fields or rules. This port matches the
executable reference; unsupported fields are ignored. The reference uses a YAML
subset parser, including inline arrays and field declarations, rather than full
YAML. The same subset is intentionally used here without a YAML dependency.

`capture_default_pattern` is parsed by the reference but never consulted by its
runtime. Pattern A is the explicit daily append workflow; pattern B is explicit
capture records. Captures are not silently redirected based on this setting.

Frontmatter defaults and required/type declarations are parsed and exposed but,
matching the reference, create_record only adds scheduled and normalizes meeting
attendees; it does not enforce required/type declarations or apply defaultValue.

Security improvement: framework schema and registry operations obey effective
blocked paths and use dedicated atomic metadata writes without note cooldown
semantics. All note mutations use the audited writer. The TypeScript management
code does not enforce effective blocked paths. Missing record templates are tolerated; blocked,
unreadable, and escaped templates are errors. Daily templates remain required.

## Interim dependency handoff

The initial pure schema tests passed before workflow integration was added.
Workflow implementation and nine runtime tests are now authored. They await the
storage agent's committed writer API before compiling and wiring Runtime; they
are not yet GREEN. HTTP acceptance, comprehensive checks, final rebasing,
version/changelog, and PR remain required before completion.

## Storage integration results

- Integrated storage interface `a2025f1` and follow-up `e16e7e7` as temporary
  dependency commits. Final PR must rebase onto the actual A/B squash commits.
- GREEN: `cargo +stable test --lib framework`: 6 passed.
- GREEN: framework runtime suite: 11 passed. This includes atomic metadata
  initialization/overwrite, registry persistence/order/replacement, concurrent
  registry updates, no metadata cooldown, template handling, maps, sample YAML,
  UTC records, source replacement, daily protections/conflicts/repair, blocked
  paths and escaped symlinks.
- Date RED: ISO `2026-02-30` initially failed; GREEN after matching JavaScript's
  day-overflow normalization while rejecting day 32 and month 13.
- HTTP RED (coordinator run, outside network sandbox):
  `cargo +stable test --test framework_parity http_framework_overlay_capture_daily_and_scope_acceptance -- --nocapture`
  failed because the pre-A transport expects an initialize request (422);
  the test remains active. HTTP acceptance requires A integration.
- Current stable Clippy gate is blocked by inherited pre-A
  `mcp::list_tools` (`unused_async_trait_impl`). C lints were corrected; the final
  gate must run without suppressions after A integration.

The in-process runtime test run intentionally excluded the separately recorded
HTTP RED case during dependency development. No test is ignored in source and
this is not a claim that the full suite passes.


Markdown output hardening RED/GREEN: a framework schema written to `.md` initially
produced zero audit rows. The regression now passes: `.md` output uses the audited
note writer, and an immediate overwrite obeys note cooldown while leaving the
original content intact. Normal YAML/JSON metadata retains reference cooldown-free
semantics. This closes an administrative route around note auditing and is an
intentional security improvement over the reference.

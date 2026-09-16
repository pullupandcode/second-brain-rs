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
blocked paths and use the audited writer; the TypeScript management code performs
unaudited raw filesystem writes. Missing record templates are tolerated; blocked,
unreadable, and escaped templates are errors. Daily templates remain required.

## Interim dependency handoff

The initial pure schema tests passed before workflow integration was added.
Workflow implementation and nine runtime tests are now authored. They await the
storage agent's committed writer API before compiling and wiring Runtime; they
are not yet GREEN. HTTP acceptance, comprehensive checks, final rebasing,
version/changelog, and PR remain required before completion.

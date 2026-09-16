# Changelog

## [Unreleased]

## [0.3.0] - 2026-09-16

- Implement atomic create/replace, frontmatter patches and marker replacements with optimistic hashes, cooldown and per-path serialization.
- Add soft deletion with collision-safe trash destinations and separate permanent deletion.
- Persist append-only audit lifecycle and successful-write provenance, recover incomplete attempts and rotate oversized audit databases at startup.
- Apply reloadable path policy, quarantine and symlink protections to every mutation; refresh the index after successful runtime writes.
- Add `[deletes].trash_path` and writer APIs for framework, capture and daily-note workflows.
- Resolve filesystem spelling before mutation policy and locking, closing case-alias bypasses on case-insensitive filesystems; serialize index snapshot refreshes.
- Intentional pre-1.0 API change: `Runtime` no longer implements `UnwindSafe`
  because shared async mutation/refresh state requires explicit panic-boundary handling.

## [0.2.0] - 2026-09-16

- Incorporate the phase-2 vault reader, parser, SQLite index, path policy and tests.
- Authenticate every protected MCP request and forward its context; expose
  structured tool results, scope failures, prompts, bounded JSON parsing and
  request/header consistency checks over stateless HTTP.
- Match reference initialization defaults and ignore unused method parameters
  before SDK decoding, preserving header binding and scoped dispatch.
- Complete 31 base/34 optional-OCR tool schemas and eight-scope discovery.
- Load/reload vault skill maps, admin diagnostics and separately scoped prompts;
  share privacy policy across readers/index and future writers, including aliases.
- NFC-normalize hard-deny comparisons before Unicode case-insensitive matching
  across platforms, including missing
  path prefixes and skill reloads, to prevent configured-denylist case and
  canonical-Unicode-alias bypasses.
  Soft ignored globs and ordinary path identity remain case-sensitive.
- Add in-memory OCR notebook/renumber queue and status contract.
- Preserve correctly rounded floating-point JSON tool arguments and correlate
  fractional or large numeric JSON-RPC IDs through the stateless transport;
  normalize input numbers to ECMAScript precision, including integers above 2^53.
- Match ECMAScript canonical numeric scalar formatting, including exponent
  thresholds, signed zero and finite-number checks; share the parser with
  framework schema parsing.
- Fix empty root/search inputs, canonical folder listings, directory errors,
  frontmatter tag order/body-only links, and escaped string/list roundtrips.
- Fail closed for JWT mode until the production verifier release; pending tools
  return errors rather than successful `not_implemented` responses.
- Update vulnerable/yanked dependency versions without advisory exceptions.
- Intentional pre-1.0 API changes: handler construction requires runtime;
  handler no longer implements unwind-safety auto traits; Scope numeric
  discriminants change while OAuth wire strings remain stable.

## [0.1.0]

Initial server skeleton, configuration, discovery and scope-filtered registry.

# Changelog

## [Unreleased]

## [0.5.0] - 2026-09-16

- Verify production RS256/ES256 JWT access tokens against configured issuer JWKS;
  enforce issuer, audience, subject, expiry, optional not-before and algorithm/key
  metadata. Development scope tokens are never accepted in JWT mode.
- Cache keys per issuer with serialized rollover, TTL expiry, bounded async HTTP,
  failed-fetch cooldown and fail-closed refresh. Use AWS-LC cryptography.
- Authenticate each dispatched MCP request asynchronously; preserve all eight
  independent scopes and allow the exact configured public Host for HTTPS proxies.
- Restore reference development-mode fallback scopes for missing headers.
- Record authentication outcomes and peer IP without bearer contents; honor
  explicit argument logging with conventional credential redaction.
- Add signed-token adversarial, live HTTP, boundary, redaction, concurrency,
  configuration and mutation-test evidence; document deployment and limitations.
- Pre-1.0 API migration: `Authenticator::authenticate` returns `AuthFuture` and must
  be awaited. `AuthContext` now includes issuer, audience and optional token ID;
  operational log entries optionally include arguments. `exp` is required as
  intentional hardening over the pinned reference's optional-exp behavior.

## [0.4.0] - 2026-09-16

- Add reference-compatible framework schemas, LYT/PARA/Zettelkasten presets, overlays, and atomic registry persistence.
- Implement framework initialization, registration, listing, validation/reload, and live composition without restart.
- Create schema-driven records with UTC filename tokens, templates, scheduled fields, and normalized meeting attendee links.
- Add dated captures and source-ID replacements with audited optimistic writes and immediate index refresh.
- Implement daily-note creation, protected-section append, and marker repair through the audited writer.
- Populate vault structure and record-type discovery; find map/index notes using effective schema folders, with English locale ordering matching the pinned reference.
- Normalize ISO date forms and overflow consistently across records, captures, and daily notes, including local timezone and daylight-saving resolution, signed extended years, and JavaScript date bounds.
- Apply effective path policy to schema, registry, template, and note access; provide a copyable LYT schema.

## [0.3.0] - 2026-09-16

- Keep incomplete audit attempts in the live store by deferring retention; reserve archive destinations without overwriting existing history and commit completion/provenance in one transaction.
- Reject trash paths that normalize to the vault root and Windows drive-prefixed vault paths.
- Preserve Unix permission bits during replacement, frontmatter and marker writes; prepare replacement files with restricted temporary permissions.
- Exercise storage replacement and path validation on Windows CI using Tokio's documented existing-file replacement behavior.

- Implement atomic create/replace, frontmatter patches and marker replacements with optimistic hashes, cooldown and per-path serialization.
- Serialize numeric frontmatter with JavaScript canonical number spelling, preserving finite values across write/read roundtrips.
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

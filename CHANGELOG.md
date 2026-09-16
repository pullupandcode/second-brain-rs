# Changelog

## [Unreleased]

## [0.2.0] - 2026-09-16

- Incorporate the phase-2 vault reader, parser, SQLite index, path policy and tests.
- Authenticate every protected MCP request and forward its context; expose
  structured tool results, scope failures, prompts, bounded JSON parsing and
  request/header consistency checks over stateless HTTP.
- Complete 31 base/34 optional-OCR tool schemas and eight-scope discovery.
- Load/reload vault skill maps, admin diagnostics and separately scoped prompts;
  share privacy policy across readers/index and future writers, including aliases.
- Add in-memory OCR notebook/renumber queue and status contract.
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

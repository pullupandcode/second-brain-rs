# second-brain-rs

A Rust Streamable HTTP MCP server for Markdown vaults, targeting
[`second-brain-mcp` v1.1.1](https://github.com/pullupandcode/second-brain-mcp/tree/48b272337a6ef7e381fd175b2ff1844c02cebd7a).
The combined **v0.5.0 implementation**, merged through #52, includes audited note mutations, frameworks,
captures, daily notes, private skill prompts, and production JWT verification.
Version **0.5.1** adds storage review fixes; **0.5.2** adds downloadable executables
and automated immutable releases. Release status, verification
and known limitations are tracked in the [delivery ledger](docs/PARITY_STATUS.md).

Download your platform archive and `SHA256SUMS` from the
[GitHub releases](https://github.com/pullupandcode/second-brain-rs/releases).
Starting with v0.5.2, releases include Linux x86-64, Windows x86-64, macOS Apple
Silicon and macOS Intel executables. Extract the archive, copy
`config.example.toml` to `config.local.toml`, configure it, then run
`./second-brain-rs --config config.local.toml` (PowerShell:
`.\second-brain-rs.exe --config config.local.toml`). No Rust installation is needed.
See [installation and release details](docs/RELEASING.md) for checksums and supported
operating-system baselines. Existing immutable releases cannot gain binary assets.

To build from source with current stable Rust:

```sh
cargo +stable build --release --locked
./target/release/second-brain-rs --config config.local.toml
```

Copy [config.example.toml](config.example.toml), set an existing vault and separate
state directory, and configure your JWT issuer and audience. Production mode
verifies RS256/ES256 signatures with cached issuer keys. Local development mode
accepts `Bearer scope=vault:read`; missing or unrecognized development scope claims
use the configured fallback scopes. Development mode is restricted to loopback
unless an operator explicitly enables its environment override.

The endpoints are `/mcp`, `/tools`, `/healthz`, and
`/.well-known/oauth-protected-resource`. MCP uses stateless JSON responses and
checks identity and scopes independently for each dispatched operation. Protocol
preflight bounds request bodies at 1,000,000 bytes, validates optional `mcp-method`
and `mcp-name` headers, and can answer protocol-only requests before authentication.

## Tools and scopes

The server has 31 base tools and three optional OCR contract tools.
Scopes are independent: `admin` does not imply any other scope.

| Scope | Tools or operations |
|---|---|
| `vault:read` | `read_note`, `list_folder`, `search`, `get_backlinks`, `get_outgoing_links`, `daily_note_get`, `find_maps`, `list_record_types`, `get_vault_structure`, `link_to_page` |
| `skills:read` | MCP `prompts/list` and `prompts/get` |
| `vault:write` | `create_note`, `replace_note`, `update_frontmatter`, `replace_section_by_marker`, `create_record` |
| `vault:delete` | `delete_note` |
| `vault:delete:hard` | `hard_delete_note` |
| `vault:capture` | `inbox_capture`, `capture_for_date` |
| `daily:append` | `daily_note_append` |
| `admin` | `list_vault_conflicts`, `daily_note_repair_markers`, `list_write_recovery_diagnostics`, `skills_list`, `skills_reload`, `framework_init`, `framework_reload`, `framework_register`, `framework_unregister`, `framework_list`, `framework_compose`; optional `ocr_notebook`, `ocr_status`, `ocr_renumber_notebook` |

`daily_note_get` can create a missing daily note from its template, even though it
uses `vault:read`; this matches the reference. Existing-note mutations require
`base_sha256` from the note's `currentSha256`, and enforce cooldown, privacy policy
and conflict quarantine. Soft deletion uses the configured trash directory;
permanent deletion requires its separate scope. Each note mutation records audit
lifecycle/provenance, with incomplete-attempt diagnostics for recovery.

## Frameworks and private prompts

Initialize LYT, PARA or Zettelkasten with `framework_init`, or copy the
[LYT starter schema](examples/vault/_meta/framework.lyt.yaml) to your vault's
`_meta/framework.yaml`. Register overlays with an optional priority and inspect
`framework_compose`. Records use schema folders, filename patterns and templates;
source-ID replacement supports repeatable captures. Daily workflows use
`Calendar/Days/YYYY-MM-DD.md` and `x/Templates/Daily Template.md`.

Configure `[skills] map_paths = ["Maps/Skills.md"]` to load linked skill notes.
Each skill has frontmatter `name` (a lowercase slug, at most 64 characters),
`description`, and a nonempty body. Maps support wikilinks, Markdown links and
`Path:` hints. Skill maps, discovered candidates and canonical targets are private
to ordinary vault access. Hard denies apply NFC Unicode normalization followed
by Unicode case-insensitive matching on every platform, including path prefixes
that do not yet exist. This deliberately also denies canonically equivalent or
differently cased distinct paths on filesystems that distinguish them.
Ordinary path identity, search identity, and soft ignored globs remain
case-sensitive. Admin diagnostics omit bodies; retrieving a prompt
requires `skills:read`. Reload updates prompts and the shared privacy policy.

## Reference limits and deliberate differences

- The index rebuilds at startup and after server mutations. External edits need
  a restart or subsequent rebuild to refresh search; background watching is not
  implemented. The `watcher_polling` setting is retained for compatibility.
- `capture_default_pattern` is parsed but unused by the reference runtime.
  Captures create records; daily appends are explicit operations.
- Framework field/default declarations are exposed, but required-field validation
  and automatic defaults are not applied. Missing record templates are tolerated;
  the daily template is required when creating or repairing a daily note.
- OCR tools only queue and inspect in-memory jobs. There is no OCR worker and no
  job persistence, matching the reference contract.
- JWT expiry is required as intentional hardening. Network/key-cache limits,
  sanitized failures, canonical-path privacy, atomic publication and stricter
  filesystem protections are documented in the scope reports.

See the [user guide](docs/USER_GUIDE.md), [authentication guide](docs/AUTHENTICATION.md),
[deployment guide](docs/DEPLOYMENT.md), and [configuration example](config.example.toml).
Development and review requirements are in [CONTRIBUTING.md](CONTRIBUTING.md).
Reference mappings and test evidence: [A](docs/parity/A.md), [B](docs/parity/B.md),
[C](docs/parity/C.md), [D](docs/parity/auth.md).

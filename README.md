# second-brain-rs

Rust port of `second-brain-mcp` v1.1.1, delivered in reviewed increments.
Version 0.4.0 implements read, write/delete, framework, capture, daily-note,
skills, and OCR-contract tools. Production JWT mode fails closed until the
verifier release. See [the parity plan](docs/PARITY_PLAN.md).

Build with current stable Rust (minimum 1.95):

```sh
cargo build --release
cargo run -- --config config.local.toml
```

Copy [config.example.toml](config.example.toml), set existing vault and state paths,
and use `auth.mode = "development"` only on loopback. For local testing, send
`Authorization: Bearer scope=vault:read`. The HTTP endpoints are `/healthz`,
`/.well-known/oauth-protected-resource`, `/tools`, and `/mcp` (stateless JSON MCP).
Every protected request is authenticated independently. Request bodies are limited
to 1,000,000 bytes; optional `mcp-method` and `mcp-name` headers must match the body.

All 31 base tools have handlers; three optional OCR tools implement the reference
job contract. Empty folder paths select the root; empty queries list indexed
notes. The index is rebuilt at startup and after successful server mutations;
external file edits require a restart to refresh search. Writes use `base_sha256`
for optimistic concurrency, enforce a per-path cooldown, and record audit/provenance
entries. Soft deletion moves notes into the configured trash; hard deletion requires
its separate `vault:delete:hard` scope.

| Scope | Purpose |
|---|---|
| `vault:read` | Notes, search, folders, links and structure |
| `skills:read` | `prompts/list` and `prompts/get` |
| `vault:write` | Note and record mutations |
| `vault:delete` | Soft deletion |
| `vault:delete:hard` | Permanent deletion |
| `vault:capture` | Capture workflows |
| `daily:append` | Daily-note append |
| `admin` | Diagnostics, skill reload, framework management, OCR |

Configure `[skills] map_paths = ["Maps/Skills.md"]` to load linked notes with
frontmatter `name` (lowercase slug, at most 64 characters) and `description`, plus
nonempty body content. Maps support wikilinks, relative Markdown links, and
`Path:` hints. Loaded and invalid candidates, configured maps, and their existing
canonical targets are private to ordinary vault access. Hard-deny patterns use
NFC Unicode normalization followed by Unicode case-insensitive matching on every
platform, including prefixes that do not yet exist. This deliberately also blocks
canonically equivalent or differently cased distinct paths on filesystems that
distinguish them. Ordinary path identity, search identity and soft ignored-glob
matching retain their original case-sensitive behavior. Admin diagnostics omit
skill bodies; prompts require the separate `skills:read` scope. Reload replaces
skills and the shared privacy policy without restart. Previously private notes
remain absent from the index until a rebuild after being unloaded.

`[ocr] enabled = true` adds `ocr_notebook`, `ocr_status`, and
`ocr_renumber_notebook` (31 base tools, 34 with OCR). Jobs receive UUIDs and UTC
timestamps and remain queued. As in the reference, this is an in-memory job
contract, not an OCR worker; jobs disappear on restart.

See [CONTRIBUTING.md](CONTRIBUTING.md) for validation and
[docs/parity/A.md](docs/parity/A.md) and [docs/parity/B.md](docs/parity/B.md)
for reference mappings and intentional
security/compatibility improvements.

Framework reference mappings and test evidence are recorded in
[scope C](docs/parity/C.md).

## Framework schemas and records

Initialize a starter schema using `framework_init` with `framework: lyt`, `para`,
or `zettel`. Creation is exclusive; `mode: overwrite` explicitly replaces an
existing schema. The default path is `_meta/framework.yaml`, configurable through
`[framework].schema_path`.

For a richer LYT starting point, copy
[examples/vault/_meta/framework.lyt.yaml](examples/vault/_meta/framework.lyt.yaml)
to your vault as `_meta/framework.yaml`. Customize its folders, filename patterns,
and template paths. Record templates that do not exist are treated as empty.

Register overlays with `framework_register` (`name`, `path`, optional integer
`priority`, default 100). Registrations persist in `_meta/schemas.json` and compose
in priority/name order. `framework_reload` reports per-overlay validation;
`framework_compose` computes the effective schema. Types may only replace an
existing definition when the overlay declares `override: true`; overlays cannot
change the base `inbox.folder`.

`create_record` expands schema filename tokens and merges optional `fields` with
record metadata. `capture_for_date` and `inbox_capture` create capture records;
`inbox_capture` with `strategy: replace_by_source_id` replaces the indexed source
note using its current hash. Captures do not modify daily notes.

`daily_note_get` reads or creates `Calendar/Days/YYYY-MM-DD.md` from
`x/Templates/Daily Template.md`. `daily_note_append` requires `base_sha256` and
appends to `daily-log` by default, or the named writable `last-light` section.
`agenda` is protected. Administrators can restore missing template marker blocks
with `daily_note_repair_markers`.

Schema/registry metadata uses atomic publication and effective path policy, with
no note cooldown. A schema output path ending in `.md` uses the audited note
writer and its cooldown policy. All note mutations use audited, hash-checked storage operations.
The reference schema subset exposes frontmatter defaults and field declarations;
record creation only synthesizes `scheduled` and normalizes meeting attendees.
It does not enforce required fields or apply schema defaults automatically.

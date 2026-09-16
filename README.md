# second-brain-rs

Rust port of `second-brain-mcp` v1.1.1, delivered in reviewed increments.
**v0.2.0 is the read/protocol/skills/OCR-contract release, not full production parity.**
Production JWT mode fails closed until the verifier release. Write, delete,
framework, capture and daily-note handlers are advertised but return tool errors
until their planned releases. See [the parity plan](docs/PARITY_PLAN.md).

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

Implemented tools: `read_note`, `list_folder`, `search`, `get_backlinks`,
`get_outgoing_links`, `list_vault_conflicts`, `link_to_page`, `get_vault_structure`
(folder data; record types arrive in v0.4), `skills_list`, `skills_reload`, and
three optional OCR tools. Empty folder paths select the root; empty queries list
indexed notes. The index is rebuilt at startup; external file edits require a
restart to refresh search.

| Scope | Purpose |
|---|---|
| `vault:read` | Notes, search, folders, links and structure |
| `skills:read` | `prompts/list` and `prompts/get` |
| `vault:write` | Note and record mutations (planned v0.3/v0.4) |
| `vault:delete` | Soft deletion (planned v0.3) |
| `vault:delete:hard` | Permanent deletion (planned v0.3) |
| `vault:capture` | Capture workflows (planned v0.4) |
| `daily:append` | Daily-note append (planned v0.4) |
| `admin` | Diagnostics, skill reload, framework management, OCR |

Configure `[skills] map_paths = ["Maps/Skills.md"]` to load linked notes with
frontmatter `name` (lowercase slug, at most 64 characters) and `description`, plus
nonempty body content. Maps support wikilinks, relative Markdown links, and
`Path:` hints. Loaded and invalid candidates, configured maps, and their existing
canonical targets are private to ordinary vault access. Admin diagnostics omit
skill bodies; prompts require the separate `skills:read` scope. Reload replaces
skills and the shared privacy policy without restart. Previously private notes
remain absent from the startup index until restart even after being unloaded.

`[ocr] enabled = true` adds `ocr_notebook`, `ocr_status`, and
`ocr_renumber_notebook` (31 base tools, 34 with OCR). Jobs receive UUIDs and UTC
timestamps and remain queued. As in the reference, this is an in-memory job
contract, not an OCR worker; jobs disappear on restart.

See [CONTRIBUTING.md](CONTRIBUTING.md) for validation and
[docs/parity/A.md](docs/parity/A.md) for reference mappings and intentional
security/compatibility improvements.

# User guide

Build with current stable Rust, copy `config.example.toml` to a machine-local
configuration file, and set an existing vault directory plus a separate state
directory. The process needs read/write access to both. Keep state outside the
vault's synchronized contents. Start with:

```sh
cargo +stable build --release --locked
./target/release/second-brain-rs --config config.local.toml
```

`GET /healthz` checks HTTP availability. Configure a Streamable HTTP MCP client
with the `/mcp` URL and a valid access token. See [authentication](AUTHENTICATION.md)
for issuer setup, the eight independent scopes, and local development mode.
`/tools` is a JSON convenience listing; MCP clients should use `tools/list`.

## Read and discover

Start with `get_vault_structure`, `list_record_types`, and `list_folder`.
An empty folder path means the vault root. `read_note` returns content, parsed
frontmatter, and `currentSha256`. `search` supports text, folder, and tag filters;
an empty query lists indexed matches. Backlink and outgoing-link tools follow
wikilinks. The index rebuilds at startup and is updated after server mutations.
The `watcher_polling` setting is retained for compatibility; automatic background
filesystem watching is not implemented. External edits require a restart to
refresh the search index. Direct note reads use the filesystem.

`security.blocked_paths` prevents ordinary reads and writes using Unicode
case-insensitive matching after NFC normalization, including nonexistent path
prefixes. This also denies canonically equivalent or differently cased distinct
paths on filesystems that distinguish them. Index exclusions
hide notes from search/list results. Configured skill maps and loaded skill files
are also protected from ordinary tools. Do not treat index-only exclusion as a
write prohibition. Symlink escapes, traversal, and quarantined conflict files
are rejected regardless of caller scope. `list_vault_conflicts` requires `admin`.

## Write safely

`create_note` creates a new Markdown file. For an existing note, read it first
and pass its `currentSha256` as `base_sha256` to the relevant mutation.
A stale hash reports a conflict; re-read and reconcile instead of retrying an
old payload. Replace, frontmatter, marker, framework record, capture, and daily
operations use the audited writer and its configured cooldown. Marker writes
only modify the named owned section.

`delete_note` requires `vault:delete` and moves a note into `deletes.trash_path`.
`hard_delete_note` requires the separate `vault:delete:hard` scope and permanently
removes it. Grant permanent deletion only to callers that need it. Deletion
accepts optimistic concurrency checks through the advertised tool schema.

## Frameworks, records, and captures

Use `framework_init` with `framework` set to `lyt`, `para`, or `zettel` to
materialize a starter schema, then inspect `framework_compose`. A copyable LYT
example is included under `examples/vault/_meta/`. `framework_register` and `framework_unregister` manage
persisted overlays; reload validates and composes them. Inspect `framework_list`
and `framework_compose` after changing files. Overlay errors are diagnostic
results and must be resolved before depending on that overlay's record types.

`create_record` applies the selected type's folder, filename and template rules,
merging supplied `fields` with `type`, `title`, and `date` metadata. It synthesizes
`scheduled` when declared and normalizes meeting attendees to wikilinks. Schema
field/default declarations remain descriptive: required fields and automatic
default values are not enforced, matching the reference. `inbox_capture` supports
create-only and `replace_by_source_id` behavior; the latter updates the existing matching capture instead of making a
new duplicate. `capture_for_date` creates dated capture records and requires
`content` and `source_client`. Captures do not modify daily notes. The configured
`capture_default_pattern` is parsed but unused by the reference runtime; choose
`daily_note_append` explicitly for inline daily capture.

`daily_note_get` uses `vault:read` and reads or creates
`Calendar/Days/YYYY-MM-DD.md`. Creation requires the existing template
`x/Templates/Daily Template.md`. Thus `vault:read` is not an absolute guarantee of
no filesystem side effects. `daily_note_append` uses `daily:append`, requires
`base_sha256`, and appends to `daily-log` by default or writable `last-light`.
The `agenda` section is protected. Repairing missing template marker blocks uses
`daily_note_repair_markers` with `admin` and `base_sha256`.
`link_to_page` returns an existing stable OCR source-page wikilink or null.
Consult `tools/list` for exact required arguments and enums.

## Skills as MCP prompts

Set `[skills] map_paths` to vault-relative Markdown maps. Maps can identify
skills through wikilinks, Markdown links, and `Path:` hints. Each skill has a
frontmatter `name`, a `description`, and a nonempty body. The name must be a
lowercase slug of at most 64 characters. `skills_list` shows per-file load
status; `skills_reload` rereads maps and updates prompt availability and privacy
policy together. Both administration tools require `admin`.

Clients with `skills:read` can use `prompts/list` and `prompts/get`. Prompt bodies
are served through MCP prompts, while ordinary vault tools cannot expose the
protected skill source files.

## OCR contract and recovery

Set `[ocr] enabled = true` to advertise `ocr_notebook`, `ocr_status`, and
`ocr_renumber_notebook`, all requiring `admin`. These implement the pinned
reference's in-memory queue/status contract. There is no OCR worker or conversion
backend, and queued jobs do not survive a restart. Enabling the flag does not
perform document recognition.

Writes record start and terminal events in the audit store. After an interrupted
operation, call `list_write_recovery_diagnostics` with `admin` to find incomplete
attempts, inspect the note and its current hash, and reconcile before retrying.
The tool reports diagnostics; it does not automatically replay incomplete
writes. Configure `[audit] retention_max_rows` to archive oversized audit stores
at startup; zero disables rotation. `archive_path` selects a separate archive
directory. Preserve archives according to your retention policy.

## Troubleshooting

- HTTP 401: verify exact issuer, audience, expiry, algorithm, JWKS URL, key ID,
  and server clock. A production server never accepts development scope tokens.
- `forbidden_scope`: obtain the specific scope; `admin` is not a wildcard.
- Stale hash or cooldown error: re-read and wait for the configured cooldown.
- Missing search result: check exclusions, conflict quarantine, and external edits.
- Missing prompt: inspect `skills_list`, then repair the file and reload.
- Public proxy request rejected: ensure `public_base_url`, Host handling, and
  proxy configuration match [deployment](DEPLOYMENT.md).

Logs default to argument hashes. Enabling `log_args` exposes potentially private
note contents despite credential-field redaction; see the authentication guide.

## Configuration reference

The example supplies every required table and value. Paths are vault-relative
where noted; `vault_path` and `state_path` identify local filesystem directories.

| Setting | Requirement or default | Behavior |
|---|---|---|
| `listen` | Required | Listener address; development mode enforces loopback unless explicitly overridden |
| `public_base_url` | Required HTTP(S) URL | Public resource identity and allowed proxy Host authority |
| `vault_path` | Required | Existing vault root; `~` expansion supported |
| `state_path` | Required | Runtime database directory; created when needed |
| `auth.mode` | `jwt` | `jwt` signature verification or explicit local `development` mode |
| `auth.audience` | Required nonempty string | Expected access-token audience |
| `auth.trusted_issuers` | Required nonempty URL list | Exact accepted issuers and relative JWKS endpoint roots |
| `auth.discovery_authorization_server` | Required HTTP(S) URL | Authorization server advertised in discovery |
| `auth.jwks_cache_ttl_seconds` | Required nonnegative integer | Successful key-cache reuse TTL; zero disables reuse |
| `auth.jwt_algorithms` | `["RS256"]` | Nonempty allowlist containing RS256 and/or ES256 |
| `auth.development_default_scopes` | `[]` | Fallback when a development header contains no known explicit scopes |
| `index.sqlite_path` | `{state_path}/index.sqlite` | Persistent index, or `:memory:` for tests |
| `index.watcher_polling` | Required Boolean | Compatibility setting; no background watcher |
| `index.ignored_globs` | Required list | Exclude matching paths from reader/index operations |
| `index.blocked_paths` | `[]` | Reader/index exclusions; does not itself forbid writes |
| `security.blocked_paths` | `[]` | Hard privacy policy for ordinary vault access and framework operations |
| `writes.cooldown_seconds` | Required nonnegative integer | Existing-note mutation cooldown; zero disables it |
| `deletes.trash_path` | `.trash/mcp` | Vault-relative soft-delete destination |
| `audit.retention_max_rows` | `0` | Startup rotation threshold; zero disables rotation |
| `audit.archive_path` | `{state_path}/audit-archive` | Archive destination |
| `framework.schema_path` | `_meta/framework.yaml` | Vault-relative base framework schema |
| `skills.map_paths` | `[]` | Vault-relative skill maps; included in effective privacy policy |
| `daily_note.capture_default_pattern` | Required `A` or `B` | Compatibility setting; runtime routing does not consult it |
| `ocr.enabled` | `false` | Advertise the three in-memory OCR job tools |
| `logging.log_args` | Required Boolean | Include redacted argument values in addition to hashes when true |

## Example mutation and audit inspection

Read a note, retain its `currentSha256`, then make a `tools/call` request such as:

```json
{
  "name": "replace_note",
  "arguments": {
    "path": "Notes/Example.md",
    "content": "# Example\nUpdated content.\n",
    "base_sha256": "<currentSha256 returned by read_note>"
  }
}
```

After an interrupted request, call `list_write_recovery_diagnostics` with empty
arguments and an `admin` token. Match the reported path and attempt identifier to
the current note before deciding whether a retry is needed. The active audit
store is `{state_path}/write-audit.sqlite`; rotated stores are retained under the
configured archive path. Audit tables are append-only through the service's SQL
guards. Audit persistence is best effort after a completed note mutation, so an
audit-store failure is logged without reporting that a successful write failed.
Do not assume the absence of an audit row proves a filesystem write did not occur.

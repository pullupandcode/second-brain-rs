# Tool reference

This reference describes v0.5.2: **31 base tools plus 3 optional OCR tools**.
[Use cases](USE_CASES.md) provides runnable workflows. Start with
[Getting started](GETTING_STARTED.md), configure paths in
[Configuration](CONFIGURATION.md), and grant scopes using
[Authentication](AUTHENTICATION.md). Local `sb_call` examples use the helper in
[Development](DEVELOPMENT.md).

## Calling tools and reading results

Send an MCP `tools/call` request with `params.name` and an `arguments` object.
Arguments use the exact **snake_case** names below; most returned fields use
**camelCase**. Advertised input schemas reject extra properties; omit optional
values rather than sending `null`. Required textual arguments generally must be
nonempty; documented exceptions are root `list_folder.path` and empty search queries.

`sb_call` prints the raw JSON-RPC response. Check `.error` even when HTTP succeeds.
On success, object payloads are at `.result.structuredContent`. Array payloads
are at `.result.structuredContent.result`. The JSON text in
`.result.content[0].text` contains the same underlying payload. For example,
`read_note`'s hash is `.result.structuredContent.currentSha256`.

A **write** result contains `path` and `resultSha256`, optional `baseSha256`, and
`deletedPath` for soft deletion. Hashes cover exact file bytes. For deletion,
`resultSha256` describes the removed content, not an empty file. A **note** result
contains `path`, `content`, `currentSha256`, and `parsed`.

Frontmatter arguments (`frontmatter`, `patch`, `fields`) accept string keys with
string, finite number, boolean, or string-array values. Nested objects, `null`,
and arrays containing other types are unsupported. YAML serialization can
reformat fields; this is not a general YAML-preserving editor.

ISO dates default to the current time when omitted. Date-only values are UTC;
datetimes without offsets use the server timezone. Filename/date fields use UTC.
Use an explicit offset or `Z` for predictable scheduling. JavaScript-compatible
ISO normalization includes day overflow (for example, February 30 rolls forward).
Legacy English/slash date spellings are outside the supported ISO contract.

Paths are vault-relative, without a leading slash or traversal. Use `.md` for note
paths. Hard privacy rules apply to ordinary reads/writes/search; soft index/ignored
patterns also deny direct reads but do not themselves block writes. Configured
skill prompts are a separately scoped surface. See
[Configuration](CONFIGURATION.md) for matching and filesystem constraints.

## Scopes and discovery

The eight scopes are independent: **`admin` does not imply the other scopes**.
Discovery returns only tools allowed by the caller's scopes and enabled features.
`skills:read` controls MCP prompts and has no `tools/call` entries. A client doing
read-modify-write usually needs both `vault:read` and `vault:write`.

## `vault:read`

### `read_note`

**Required:** `path` (string).

**Optional:** none.

Read the current file directly. Returns a **note** object: `path`, full `content`, `currentSha256`, and `parsed` (`frontmatter`, `body`, optional `title`/`sourceId`, `tags`, `aliases`, `outgoingLinks`).

Use `currentSha256` from a fresh read for optimistic mutations. Soft ignored paths and hard-blocked paths both reject direct reads.

### `list_folder`

**Required:** `path` (string).

**Optional:** `recursive` (boolean).

Returns an array of `{path,type}` entries, where type is `file` or `directory`.

`path: ""` selects the vault root. `recursive` defaults to `false`; recursive mode traverses directories and returns Markdown files, rather than emitting directory entries. Ignored/blocked children and symlinks are skipped.

### `search`

**Required:** `query` (string).

**Optional:** `filters` (object).

Returns an array of hits with `path`, optional `title`, `tags`, `aliases`, and `currentSha256`.

`query: ""` lists indexed notes. Nonempty queries use SQLite FTS syntax; this is not semantic/vector search. `filters` supports string `folder` and `tag` values (tag without `#`). Search uses the index, not a fresh filesystem read; external edits require restart to guarantee a refresh.

### `get_backlinks`

**Required:** `path` (string).

**Optional:** none.

Returns `{backlinks: [sourcePath, ...]}` from the index.

The input matches stored wikilink targets exactly. For a link `[[Notes/Plan]]`, query `Notes/Plan`, not automatically `Notes/Plan.md`. This does not resolve aliases or infer missing extensions.

### `get_outgoing_links`

**Required:** `path` (string).

**Optional:** none.

Returns `{outgoingLinks: [target, ...]}` for the indexed source note.

Use the source file path, including `.md`. Targets are parsed wikilink strings; a target need not be an existing note. This is not a general web-link crawler.

### `daily_note_get`

**Required:** none.

**Optional:** `date` (string).

Returns a **note** object for `Calendar/Days/{UTC date}.md`. Creates a missing note by copying `x/Templates/Daily Template.md`.

**This `vault:read` tool can create a file.** The daily template must exist and be readable when creation is needed. Existing daily notes can be read without the template. Template text is copied; template variables are not expanded. No framework schema is required for daily tools.

### `find_maps`

**Required:** none.

**Optional:** `topic` (string).

Returns `{maps: [searchHit, ...]}` using the effective framework schema.

`topic` defaults to an empty query. Searches folders of types whose name, description, or folder contains `map` or `index` (case-insensitive substring); deduplicates and sorts paths. Requires a readable, composable base schema and overlays. A schema without such types yields no maps.

### `list_record_types`

**Required:** none.

**Optional:** none.

Returns `{recordTypes: [{name,folder,description?}, ...]}` from the effective schema.

Requires the base schema and all registered overlays to compose successfully. Names use English locale ordering.

### `get_vault_structure`

**Required:** none.

**Optional:** none.

Returns `{folders: [folderEntry, ...], recordTypes: [recordType, ...]}`.

`folders` is the nonrecursive root listing and can include root Markdown files as well as directories. Requires a composable framework schema even when you only want folder data; use `list_folder` for schema-independent browsing.

### `link_to_page`

**Required:** `notebook` (string); `page_uuid` (string).

**Optional:** none.

Returns `{link: "[[path|page_uuid]]"}` or `{link: null}`.

Looks up indexed frontmatter `source_id` equal to `rmpage:<notebook>:<page_uuid>`. A leading `rmnotebook:` is stripped from `notebook`. Does not run OCR or create a note; returns null when no visible indexed match exists.

## `vault:write`

### `create_note`

**Required:** `path` (string); `content` (string).

**Optional:** `frontmatter` (object).

Creates a new file and returns a **write** result. Parent directories are created as needed.

Fails with `path_exists` instead of overwriting. `content` must be nonempty. Without `frontmatter`, content is written as supplied; with `frontmatter`, those fields are serialized before the supplied content. Supply body-only content when using that argument to avoid a second frontmatter block.

### `replace_note`

**Required:** `path` (string); `content` (string); `base_sha256` (string).

**Optional:** `frontmatter` (object).

Replaces the complete note and returns a **write** result.

Requires the current hash and respects cooldown. Existing frontmatter is not automatically retained: send the complete desired file in `content`, or body-only `content` plus the desired `frontmatter`. `content` must be nonempty.

### `update_frontmatter`

**Required:** `path` (string); `patch` (object); `base_sha256` (string).

**Optional:** none.

Merges the supplied keys into parsed frontmatter while preserving the parsed body; returns a **write** result.

Values use the supported frontmatter types below. There is no null/delete-key patch operation. Unmentioned keys remain; formatting/key order can change during serialization. Use full replacement if you need to remove a field or preserve unsupported YAML constructs.

### `replace_section_by_marker`

**Required:** `path` (string); `marker_name` (string); `content` (string); `base_sha256` (string).

**Optional:** none.

Replaces text between the first matching marker pair, retaining markers and surrounding content; returns a **write** result.

For `marker_name: "summary"`, the exact markers are `<!-- mcp:section summary start -->` and `<!-- mcp:section summary end -->`. Missing or reversed markers produce `markers_missing`. This replaces, rather than appends, the section. Nonempty content is required. Daily-section protection belongs to `daily_note_append`; a caller with generic `vault:write` can edit the underlying note.

### `create_record`

**Required:** `type` (string); `title` (string).

**Optional:** `date` (string); `body` (string); `fields` (object).

Creates a schema-driven note and returns a **write** result.

Requires a defined `type` and composable framework schema. `date` defaults to now; `body` defaults to empty and is appended after an optional template. A record can contain only generated frontmatter when both body and template are absent. Missing record templates are tolerated; blocked/unreadable templates are errors. Adds `type`, `title`, and UTC `date`, then overlays `fields` (which can replace those metadata values). A declared `scheduled` field is synthesized if not explicitly supplied; `meeting` attendee strings/lists become wikilinks. Other defaults and required/type declarations are descriptive, not applied/enforced. Filename tokens and examples are in [Use cases](USE_CASES.md#frameworks-and-records).

## `vault:delete`

### `delete_note`

**Required:** `path` (string); `base_sha256` (string).

**Optional:** none.

Moves the note into configured `[deletes].trash_path` and returns a **write** result including `deletedPath`.

Requires a current hash and cooldown. Preserves the original relative path beneath trash; a collision selects a unique destination instead of overwriting older trash. Trash is not automatically excluded from reads/search: configure matching ignored/soft-blocked patterns. There is no restore tool; restore through an ordinary reviewed filesystem operation. An interrupted move can leave both paths; see recovery guidance.

## `vault:delete:hard`

### `hard_delete_note`

**Required:** `path` (string); `base_sha256` (string).

**Optional:** none.

Permanently removes the target and returns a **write** result.

Requires the separate `vault:delete:hard` scope, current hash and cooldown. No trash copy or undo is created. A soft-ignored trash path can still be targeted by writer operations; hard privacy rules continue to apply.

## `vault:capture`

### `inbox_capture`

**Required:** `content` (string); `source_client` (string).

**Optional:** `date` (string); `source_id` (string); `capture_type` (string); `title` (string); `strategy` (string; `create`, `replace_by_source_id`).

Creates a `capture` record, or replaces an indexed record selected by `source_id`; returns a **write** result.

`strategy` defaults to `create`; `title` defaults to `Capture` for creation and `date` to now. `replace_by_source_id` requires `source_id`; it reads the matching note and supplies its current hash internally. If no indexed match exists, it creates a capture instead. Matching uses source ID alone, not `(source_client, source_id)`, so use globally unique IDs. Replacement retains the path and uses the supplied title or existing parsed title, but replaces body/frontmatter with current capture metadata rather than merging arbitrary old fields. The index does not enforce unique source IDs. Requires schema composition even for replacement; new captures require a `capture` type. Soft-hidden/external/unindexed notes are not a reliable deduplication source.

### `capture_for_date`

**Required:** `content` (string); `source_client` (string).

**Optional:** `date` (string); `source_id` (string); `capture_type` (string); `title` (string).

Creates a new framework `capture` record with `source_client` and optional source/category metadata; returns a **write** result.

Always creates; it has no `strategy` argument and does not deduplicate just because `source_id` is present. `title` defaults to `Capture`, `date` to now. The schema controls folder/filename, so a date argument alone does not ensure a dated filename. Requires a `capture` type. Does not append to a daily note. `[daily_note].capture_default_pattern` does not redirect either capture tool.

## `daily:append`

### `daily_note_append`

**Required:** `content` (string); `base_sha256` (string).

**Optional:** `date` (string); `section` (string).

Appends content within a writable daily section and returns the updated **note** object, including its new `currentSha256`.

The daily note must already exist. `date` defaults to now and selects the UTC calendar day; `section` defaults to `daily-log`. Supported sections: `daily-log` → marker `daily-captures`; `last-light` → `last-light-summary`; `agenda` exists but is not writable by this tool. Requires the current daily-note hash and cooldown. Unknown sections produce `section_missing`; agenda gives `section_not_writable`; absent marker pairs give `markers_missing`.

## `skills:read`

No tools require this scope. Use MCP `prompts/list` and `prompts/get` to retrieve validated skill instructions. `prompts/get` takes `{ "name": "skill-slug" }`; prompts do not declare arguments. Administrative `skills_list` / `skills_reload` belong to `admin`, not this scope. See [private skill prompts](USE_CASES.md#private-skill-prompts).

## `admin`

### `list_vault_conflicts`

**Required:** none.

**Optional:** none.

Returns `{conflicts: [{canonical,conflicts:[path,...]}, ...]}` from the index.

Reports `.sync-conflict-*` siblings, including conflicts in soft-excluded areas; hard-blocked subtrees are omitted. Canonical write quarantine is a startup snapshot. New/resolved external conflicts require restart for quarantine to agree with disk; a later index rebuild may report a conflict without adding it to writer quarantine.

### `list_write_recovery_diagnostics`

**Required:** none.

**Optional:** none.

Returns `{incompleteWrites: [...]}` for started attempts with no succeeded/failed terminal event, newest first (up to 100).

Each entry has `attemptId`, `operation`, `path`, `metadata`, `startedAt`, and optional `baseSha256`. An incomplete attempt is ambiguous, not proof that the file was unchanged. This tool does not replay, repair, acknowledge or roll back writes. The audit store excludes note content. `list_recent_writes` is an internal Rust method, **not an MCP tool**.

### `skills_list`

**Required:** none.

**Optional:** none.

Returns `{mapPaths:[...], skills:[status,...]}` diagnostics.

Statuses include `path`, `status` (`loaded`/`error`), optional `name`, `description`, and sanitized `error`. Does not return skill bodies and does not refresh files. The array can include missing/invalid candidates; it is not the same as the prompt list.

### `skills_reload`

**Required:** none.

**Optional:** none.

Reloads configured skill maps and directly linked candidates, swaps prompt definitions/privacy policy, and returns the same diagnostics shape as `skills_list`.

Uses configured map paths; it does not reload the server TOML. New private skill paths become blocked to ordinary vault access immediately. Unloaded notes can remain absent from the index until restart or a later successful rebuild. Invalid candidates remain private. Skill loading intentionally permits configured skill notes under `security.blocked_paths`; exposing them still requires `skills:read`.

### `framework_init`

**Required:** `framework` (string; `lyt`, `para`, `zettel`).

**Optional:** `output_path` (string); `mode` (string; `create`, `overwrite`).

Writes a starter schema and returns `{path,framework,created,overwritten}`.

`framework` is `lyt`, `para`, or `zettel`; `mode` defaults to `create` (fails if present), with `overwrite` explicitly replacing it. `output_path` defaults to configured `[framework].schema_path`, normally `_meta/framework.yaml`. A custom output path does not change the configured active path. Only schema metadata is created: no templates or notes are materialized. Normal YAML metadata writes have no note cooldown or note-audit entry; `.md` output uses the audited note writer.

### `framework_register`

**Required:** `name` (string); `path` (string).

**Optional:** `priority` (integer).

Persists a named overlay registration in `_meta/schemas.json` and returns the sorted registration array, each with `status: "registered"`.

`priority` defaults to 100; lower values load first, with English locale name ordering as the tie-break. Re-registering a name replaces that registration. Does not prove the file exists or is a valid overlay; run `framework_reload` and then `framework_compose`.

### `framework_unregister`

**Required:** `name` (string).

**Optional:** none.

Removes a registration by name and returns `{removed: true|false}`.

Idempotent for an absent name. Does not delete the overlay file or notes created from it.

### `framework_list`

**Required:** none.

**Optional:** none.

Returns registrations as an array of `{name,path,priority,status:"registered"}`.

A registration status is not a schema validation result. Use reload for per-file diagnostics and compose for the effective schema.

### `framework_reload`

**Required:** none.

**Optional:** none.

Returns `{ok,overlays:[...]}`, with each registration marked `loaded` or `error` and optional error text.

Checks registered files by reading/parsing them. Does not validate the base schema or all cross-overlay composition rules; `ok: true` is not a substitute for `framework_compose`. Framework composition already reads files on each invocation, so ordinary schema edits do not need a restart.

### `framework_compose`

**Required:** none.

**Optional:** none.

Returns the effective schema object, including `version`, `schemaKind`, `framework`, `types`, and applicable metadata/preset/inbox fields.

Loads the configured base then registered overlays in priority/name order. Duplicate type names require top-level `override: true` in the overlay; a replacement is the whole type definition, not a deep merge. Overlays cannot change `inbox.folder`. `extends` is descriptive: composition uses registered overlays, not automatic inheritance resolution. YAML support is the documented subset, not arbitrary YAML.

### `daily_note_repair_markers`

**Required:** `base_sha256` (string).

**Optional:** `date` (string).

Appends missing configured marker blocks found in the daily template, then returns the **note** object.

Requires an existing daily note and readable `x/Templates/Daily Template.md`. Requires `base_sha256`; if no blocks need adding it returns unchanged without a write/hash comparison. Copies available complete template blocks, including agenda. It does not infer missing content, fix all malformed/duplicated markers, or overwrite existing sections.

### `ocr_notebook`

**Required:** `identifier` (string).

**Optional:** `pages` (integer[]); `force` (boolean).

Queues an in-memory job and returns `{job_id,state:"queued",type:"notebook",queued_at}`.

Only registered when `[ocr].enabled = true`. `pages` is an integer array and `force` a boolean; both are optional and retained as supplied. Every call creates a new job. There is no OCR worker, notebook ingestion, automatic page-note creation, or completion transition.

### `ocr_status`

**Required:** `job_id` (string).

**Optional:** none.

Returns the stored job `{id,type,state,queuedAt,updatedAt,input}`.

Requires a job ID returned by a queue call in the current process. Unknown/restarted jobs give `job_missing`. Note the camelCase timestamp fields here versus `queued_at` in queue responses. Jobs remain queued and disappear on restart.

### `ocr_renumber_notebook`

**Required:** `notebook_id` (string).

**Optional:** none.

Queues an in-memory renumber job and returns `{job_id,state:"queued",type:"renumber",queued_at}`.

Optional OCR feature, admin scope. Records the request only; it does not renumber pages or rewrite notebook files.

## Errors and retry decisions

| Signal | Meaning and next step |
|---|---|
| JSON-RPC `-32003`, `forbidden_scope` | Obtain a token with the tool's specific scope. |
| JSON-RPC `-32601` | Unknown tool, including OCR tools when disabled. |
| JSON-RPC `-32602` | Invalid arguments or a classified operation failure; inspect message and optional `error.data`. |
| `retryable_conflict` | Stale hash or file cooldown. Read current content, merge deliberately, wait for cooldown if needed, then submit the new hash. A stale-hash error can include `currentSha256`. |
| `path_exists` / `path_missing` | Creation collided or the target is absent; inspect the path before choosing another action. |
| `path_blocked` / `path_quarantined` / `invalid_path` | Respect privacy, resolve sync conflicts, or correct the vault-relative path. Do not retry with aliases to bypass a guard. |
| `markers_missing` | Inspect the exact marker names/order; daily repair can copy missing blocks from its template. |
| `section_missing` / `section_not_writable` | Choose a configured writable daily section. |
| `job_missing` | The job ID is unknown or was lost on restart. |
| `write_failed` / internal error | Inspect server diagnostics and disk state before retrying a mutation. |

A lost response or incomplete audit record does not prove a mutation failed.
[Recovery use cases](USE_CASES.md#recovery-and-conflict-diagnostics) explains how
to inspect state before retrying. Successful writes trigger a full index rebuild;
if that refresh fails, the mutation still succeeds and the server logs the refresh
failure. There is no background filesystem watcher and no public “rebuild index”
tool. Restart to guarantee external filesystem changes are indexed.

The authoritative input-schema fixture is
[`v1.1.1-tool-schemas.json`](../tests/fixtures/v1.1.1-tool-schemas.json); scope mapping
is in [`registry.rs`](../src/tools/registry.rs).

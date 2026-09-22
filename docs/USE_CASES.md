# Practical use cases

These workflows target v0.5.2. Follow [Getting started](GETTING_STARTED.md), then
load the `sb_call` shell helper from [Development](DEVELOPMENT.md). It accepts a
tool name and a JSON arguments object and prints the raw JSON-RPC response.
Examples use `jq` to construct JSON safely and extract returned hashes.
[Authentication](AUTHENTICATION.md) explains the eight independent scopes;
[Configuration](CONFIGURATION.md) covers vault paths, exclusions and cooldown.
The complete argument and result reference is [Tools](TOOLS.md).

Use a disposable vault for these examples. Creation intentionally fails when a
path already exists, so repeated runs can need different titles/paths. Grant only
the scopes needed for the workflow: an `admin` token does not automatically grant
read, write, capture, delete or prompt access.

An HTTP 200 response can contain a JSON-RPC `.error`. Inspect it and stop on errors;
do not treat successful `curl` execution as a successful tool call. Object results
are at `.result.structuredContent`; array results are wrapped in
`.result.structuredContent.result`.

## Read, search and follow links

With `vault:write`, create two small sample notes. A reader-only client can instead
use existing notes and adapt the paths.

```sh
sb_call create_note '{"path":"Notes/Plan.md","content":"# Plan\n\nShip a small prototype. #planning","frontmatter":{"title":"Plan","tags":["planning"],"status":"draft"}}'
sb_call create_note '{"path":"Notes/Review.md","content":"# Review\n\nReview [[Notes/Plan]] before the next meeting."}'
```

With `vault:read`, browse the root, list Markdown files recursively, read a current
file, then search the index:

```sh
sb_call list_folder '{"path":"","recursive":false}'
sb_call list_folder '{"path":"Notes","recursive":true}'
sb_call read_note '{"path":"Notes/Plan.md"}'
sb_call search '{"query":"prototype","filters":{"folder":"Notes","tag":"planning"}}'
sb_call search '{"query":"","filters":{"folder":"Notes"}}'
```

`read_note` returns the full content, parsed frontmatter/body and `currentSha256`.
Search returns metadata/hashes, not full note bodies; read a chosen hit to inspect
it. Queries use SQLite full-text syntax, not semantic similarity. An empty query
lists indexed notes. The `tag` filter has no leading `#`.

```sh
sb_call get_backlinks '{"path":"Notes/Plan"}'
sb_call get_outgoing_links '{"path":"Notes/Review.md"}'
```

Backlinks match the stored wikilink target exactly: `[[Notes/Plan]]` uses
`Notes/Plan`, while the outgoing-links call uses the source filename with `.md`.
Links do not guarantee that target files exist, and there is no automatic alias
or extension resolution in the backlink query.

The server rebuilds its index at startup and after successful server mutations.
External editor/sync changes have no background watcher; restart to guarantee
fresh search and links. A refresh failure is logged without undoing an already
successful write. Rebuilding after every write also means large vaults incur more
write latency.

## Safely change a note

This workflow needs `vault:read` plus `vault:write`. Hashes describe exact on-disk
bytes, including frontmatter. Read, review the content, apply an intentional
change, and pass that read's `currentSha256` as **`base_sha256`**.

Set this shell value to the server's `[writes].cooldown_seconds` before following
the examples. Wait that long after a creation/replacement before another change;
a fresh hash alone does not bypass cooldown.

```sh
SB_COOLDOWN_SECONDS=2
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call read_note '{"path":"Notes/Plan.md"}' | jq -er '.result.structuredContent.currentSha256')
sb_call update_frontmatter "$(jq -nc --arg hash "$SB_HASH" '{path:"Notes/Plan.md",patch:{status:"active",reviewed:true},base_sha256:$hash}')"
```

This merges keys; unmentioned fields remain. Supported values are strings,
finite numbers, booleans and arrays of strings. A `null` patch does not delete a
key. Frontmatter can be reformatted and reordered. To remove keys or replace the
whole document, supply the desired complete content:

```sh
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call read_note '{"path":"Notes/Plan.md"}' | jq -er '.result.structuredContent.currentSha256')
sb_call replace_note "$(jq -nc --arg hash "$SB_HASH" '{path:"Notes/Plan.md",content:"# Plan\n\nShip and review the prototype.",frontmatter:{title:"Plan",status:"active",tags:["planning"]},base_sha256:$hash}')"
```

When `frontmatter` is provided, `content` should contain only the body. When it is
omitted, `content` must contain the complete desired note, including any
frontmatter to keep. Replacement does not automatically retain previous metadata.

A successful mutation returns `resultSha256`. For the next edit, read again so
you also see any intervening human changes. On `retryable_conflict`, distinguish a
stale hash from cooldown, reread and merge deliberately. Do not blindly overwrite
with the hash from an error response. If a response is lost, inspect disk/audit
state before retrying: the first operation may already have succeeded.

### Change only a marked section

Create a note with one exact marker pair:

```sh
sb_call create_note '{"path":"Notes/Status.md","content":"# Status\n\nHuman-owned introduction.\n\n<!-- mcp:section summary start -->\nNo update yet.\n<!-- mcp:section summary end -->\n\nHuman-owned closing notes."}'
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call read_note '{"path":"Notes/Status.md"}' | jq -er '.result.structuredContent.currentSha256')
sb_call replace_section_by_marker "$(jq -nc --arg hash "$SB_HASH" '{path:"Notes/Status.md",marker_name:"summary",content:"Prototype shipped; review is next.",base_sha256:$hash}')"
```

The surrounding text and markers remain. This operation **replaces** the section;
it does not append. Marker spelling and ordering must match, and content must be
nonempty. There is no `append_to_section` or `insert_under_heading` tool.

## Move to trash or permanently delete

Deletion has separate scopes. `vault:write` does not grant either one.
Use `vault:read` to inspect a disposable note, then `vault:delete` to move it to
trash:

```sh
sb_call create_note '{"path":"Scratch/trash-example.md","content":"Disposable trash example."}'
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call read_note '{"path":"Scratch/trash-example.md"}' | jq -er '.result.structuredContent.currentSha256')
sb_call delete_note "$(jq -nc --arg hash "$SB_HASH" '{path:"Scratch/trash-example.md",base_sha256:$hash}')"
```

Read the returned `deletedPath`; a collision can change its final filename. The
default root is `.trash/mcp`, and a note's relative directory structure is retained.
Trash is not automatically private or hidden: keep the configured destination in
`index.ignored_globs` or `index.blocked_paths` if it should disappear from browsing
and search. There is no restore tool; restore locally after inspecting both paths.
An interrupted trash move can leave both the source and the trash copy.

For a separate, explicitly disposable note, `vault:delete:hard` removes it without
a trash copy:

```sh
sb_call create_note '{"path":"Scratch/permanent-example.md","content":"Disposable permanent-delete example."}'
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call read_note '{"path":"Scratch/permanent-example.md"}' | jq -er '.result.structuredContent.currentSha256')
sb_call hard_delete_note "$(jq -nc --arg hash "$SB_HASH" '{path:"Scratch/permanent-example.md",base_sha256:$hash}')"
```

Both deletion tools require a current hash and enforce cooldown, hard privacy
rules and conflict quarantine. Deletion's `resultSha256` is the hash of the removed
content. Permanent deletion has no server undo.

## Frameworks and records

An administrator can initialize a base schema. This writes schema metadata; it
does not populate note folders or install templates.

```sh
sb_call framework_init '{"framework":"lyt"}'
sb_call framework_compose '{}'
sb_call list_record_types '{}'
sb_call get_vault_structure '{}'
```

The last two calls require `vault:read`; the first two require `admin`. The default
base path is `_meta/framework.yaml`. `mode: "create"` is the default and protects
an existing schema; `mode: "overwrite"` explicitly replaces it. A custom
`output_path` creates a different file but does not change the configured active
schema path.

| Preset | Types and default folders |
|---|---|
| `lyt` | `meeting` → `Calendar/Records/Meetings`; `capture` → `Calendar/Records/Captures`; `person` → `Atlas/Dots/People`; `project` → `Efforts/Projects/Active`; `map` → `Atlas/Maps` |
| `para` | `project` → `Projects`; `area` → `Areas`; `resource` → `Resources`; `archive` → `Archives` |
| `zettel` | `fleeting_note` → `Fleeting`; `literature_note` → `Literature`; `permanent_note` → `Permanent` |

Starter schemas use `{title}.md` filenames. A date argument alone does not make
the filename date-based. For example, with `vault:write`:

```sh
sb_call create_record '{"type":"project","title":"Prototype","date":"2026-09-21","body":"# Prototype\n\nBuild one useful end-to-end workflow.","fields":{"status":"active"}}'
sb_call create_record '{"type":"map","title":"Planning","body":"# Planning\n\n- [[Notes/Plan]]"}'
sb_call find_maps '{"topic":"Planning"}'
```

Under LYT, these create `Efforts/Projects/Active/Prototype.md` and
`Atlas/Maps/Planning.md`. `find_maps` requires `vault:read`; it searches schema
folders identified by `map`/`index` in their type name, description or folder.
All framework discovery/record operations require a readable base and valid
composition; `list_folder` remains available without a framework schema.

### Add an overlay

Save this file locally as `_meta/work.yaml` under the vault:

```yaml
version: 1
schema_kind: overlay
name: work
types:
  decision:
    description: A decision with a date and rationale.
    folder: Work/Decisions
    filename: "{date:YYYY-MM-DD} {title}.md"
    frontmatter:
      status: draft
```

Register and validate it with `admin`:

```sh
sb_call framework_register '{"name":"work","path":"_meta/work.yaml","priority":50}'
sb_call framework_list '{}'
sb_call framework_reload '{}'
sb_call framework_compose '{}'
```

The registry persists in `_meta/schemas.json`. Lower priorities load first;
equal priorities use English locale name ordering. Re-registering a name updates
its entry. Registration alone does not validate the file. Reload reports
per-overlay parse results; compose also validates the base and interactions
between overlays. Edits are read on each composition, so a normal schema edit
does not require restarting the server.

Create the new type with `vault:write`:

```sh
sb_call create_record '{"type":"decision","title":"Use a small release","date":"2026-09-21T14:30:00Z","body":"# Decision\n\nShip the smallest complete workflow first.","fields":{"status":"accepted"}}'
```

This creates `Work/Decisions/2026-09-21 Use a small release.md`. To replace an
existing type definition, an overlay needs top-level `override: true`. It replaces
the **whole** definition; include the folder, filename, template and declarations
you want to keep. Overlays cannot change `inbox.folder`. An `extends` label does
not cause another file to load; registration determines composition.

To stop applying the overlay without deleting its file or records:

```sh
sb_call framework_unregister '{"name":"work"}'
```

Supported filename tokens are `{title}`, `{date:YYYY}`, `{date:YYYY-MM}`,
`{date:YYYY-MM-DD}`, `{date:YYYY-MM-DD HH-mm}` and `{quarter}`. Date parts are UTC;
prefer `Z`/explicit offsets for datetimes. Invalid filename characters in titles
become `-` and surrounding whitespace is trimmed.

A type's optional `template` is a vault-relative Markdown path. Its text precedes
`body`, separated by a blank line; missing record templates are tolerated, while
blocked/unreadable templates are errors. Omitting both body and template creates
a record containing only generated frontmatter. Template variables are not expanded.

Record creation adds frontmatter `type`, `title` and UTC `date`; explicit `fields`
can override these metadata values. Most schema defaults and required/type
constraints are **descriptive**, not automatically filled or enforced. The
`status: draft` declaration above is exposed as a default in composition, but
creation only writes it when you supply a field. The special `scheduled`
declaration generates a scheduled string when absent from `fields`; a `meeting`
record also normalizes `attendees` strings/string arrays into wikilinks. Use the
[sample LYT schema](../examples/vault/_meta/framework.lyt.yaml) for those declarations.
The parser supports a YAML subset, not anchors, arbitrary block scalars or all YAML types.

## Capture notes from an integration

Capture tools need `vault:capture`. New captures require an effective `capture`
type: LYT includes one; PARA/Zettelkasten need an overlay defining it. The caller
does not supply a base hash for these tools.

```sh
sb_call capture_for_date '{"content":"A useful observation from the prototype review.","source_client":"review-import","source_id":"review-import:item:42","capture_type":"observation","title":"Prototype review 42","date":"2026-09-21T14:30:00Z"}'
```

This creates a capture file, not a daily-log entry. The LYT starter path is
`Calendar/Records/Captures/Prototype review 42.md`. Use a schema filename token to
include dates if desired. `capture_for_date` always creates; `source_id` by itself
does not enable deduplication. Reusing the same title can therefore hit `path_exists`.

For an integration that intentionally replaces its prior capture:

```sh
sleep "$SB_COOLDOWN_SECONDS"
sb_call inbox_capture '{"content":"Updated observation after the second review.","source_client":"review-import","source_id":"review-import:item:42","capture_type":"observation","title":"Prototype review 42","date":"2026-09-21T15:30:00Z","strategy":"replace_by_source_id"}'
```

The server finds an indexed note by `source_id`, reads its hash, and replaces it
at the same path using optimistic concurrency. It creates a new capture if no
match is found. IDs must be unique across all clients: lookup is not namespaced
by `source_client`, and no uniqueness constraint chooses safely among duplicates.
An externally added or soft-excluded note may not be indexed and therefore cannot
be relied on for deduplication. Source replacement still obeys cooldown and privacy.

Replacement uses current capture fields and replaces arbitrary old frontmatter;
it is not a metadata merge. With no title argument, the existing parsed title is
used when replacing; creation defaults to `Capture`. A supplied date affects
metadata, not the existing filename. The default strategy is `create`.
`[daily_note].capture_default_pattern` is retained for reference compatibility but
does not route capture calls to daily notes. Use `daily_note_append` explicitly
for an inline daily log.

## Daily notes and writable sections

Install this exact template as `x/Templates/Daily Template.md` in the vault:

```markdown
# Daily note

## Agenda
<!-- mcp:section agenda start -->
<!-- mcp:section agenda end -->

## Daily log
<!-- mcp:section daily-captures start -->
<!-- mcp:section daily-captures end -->

## Last light
<!-- mcp:section last-light-summary start -->
<!-- mcp:section last-light-summary end -->
```

The current daily path and template path are fixed. The resulting note is
`Calendar/Days/2026-09-21.md`; a framework overlay or capture-pattern setting does
not change it. The template is copied literally. No base framework schema is
needed for daily tools.

```sh
sb_call daily_note_get '{"date":"2026-09-21"}'
```

**`daily_note_get` requires only `vault:read` and creates the note if missing.**
Account for that behavior when granting a supposedly read-only integration access.
An existing daily note is simply returned; creation needs the template and obeys
write privacy/auditing guards. The returned `currentSha256` is used for appends.

With `daily:append`, append a log entry using the latest hash:

```sh
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call daily_note_get '{"date":"2026-09-21"}' | jq -er '.result.structuredContent.currentSha256')
sb_call daily_note_append "$(jq -nc --arg hash "$SB_HASH" '{date:"2026-09-21",content:"- Reviewed the prototype and chose the next release.",base_sha256:$hash,section:"daily-log"}')"
```

The section names are API names, not marker names:

| `section` argument | Marker name | Appending allowed? |
|---|---|---|
| `daily-log` (default) | `daily-captures` | Yes |
| `last-light` | `last-light-summary` | Yes |
| `agenda` | `agenda` | No |

A successful append returns the full updated note and new `currentSha256`.
The note must exist before append. Agenda protection applies to this scoped daily
API; it does not remove a generic `vault:write` caller's ability to edit the file.

If a note lacks a configured marker pair, an administrator can copy missing
complete blocks from the template:

```sh
sleep "$SB_COOLDOWN_SECONDS"
SB_HASH=$(sb_call daily_note_get '{"date":"2026-09-21"}' | jq -er '.result.structuredContent.currentSha256')
sb_call daily_note_repair_markers "$(jq -nc --arg hash "$SB_HASH" '{date:"2026-09-21",base_sha256:$hash}')"
```

Repair appends available missing blocks; it does not reconstruct lost text or
resolve every malformed/reversed/duplicated marker. Inspect partial marker damage
before applying it. If nothing needs adding, repair returns the existing note
without a mutation or stale-hash comparison. The template must still be readable.

## Private skill prompts

Skills are validated Markdown instructions exposed as MCP **prompts**, not tools.
Prepare files locally, because configured maps/candidates become private to
ordinary vault read/write tools.

Save `Maps/Skills.md`:

```markdown
# Skills
- [[Skills/Research Coach]]
```

Save `Skills/Research Coach.md`:

```markdown
---
name: research-coach
description: Review a research plan and identify missing evidence.
---
Ask what decision the research must support. Separate observations from assumptions,
and suggest the next source that would resolve the largest uncertainty.
```

Configure the map, then start/restart the server:

```toml
[skills]
map_paths = ["Maps/Skills.md"]
```

A skill needs a lowercase ASCII slug starting with a letter/digit (remaining
characters may also include `_` or `-`, at most 64 characters), a nonempty
single-line description and a nonempty body. The loader follows direct map links
or path hints, not arbitrary transitive note links. Duplicate skill names are
resolved deterministically by candidate path order; use unique names to avoid
ambiguity. Invalid candidates are reported and remain private.

With `admin`, inspect and reload edited skill files:

```sh
sb_call skills_list '{}'
sb_call skills_reload '{}'
```

Changing file contents takes effect on reload; changing TOML `map_paths` requires
restarting with the new configuration. Admin diagnostics expose statuses, not
prompt bodies. With **`skills:read`**, send these JSON-RPC request bodies to the
MCP endpoint (see [Development](DEVELOPMENT.md) for the authenticated transport):

```json
{"jsonrpc":"2.0","id":101,"method":"prompts/list","params":{}}
```

```json
{"jsonrpc":"2.0","id":102,"method":"prompts/get","params":{"name":"research-coach"}}
```

`prompts/get` returns the skill description and one user-message text containing
the skill body; it does not execute an agent or invoke tools. There is no tool
named `get_prompt`. Configured skills may deliberately live beneath
`security.blocked_paths`: ordinary vault access remains denied while
`skills:read` permits the configured prompt surface. Grant that scope only to
clients meant to receive the instructions.

## Recovery and conflict diagnostics

With `admin`, inspect sync conflicts and unfinished write attempts:

```sh
sb_call list_vault_conflicts '{}'
sb_call list_write_recovery_diagnostics '{}'
```

Conflict groups contain a canonical note and its `.sync-conflict-*` siblings.
Conflict notes are quarantined, and canonical quarantine is established at
startup. **New or resolved external conflicts require restart to refresh writer
quarantine.** An index refresh may list a new conflict before writer enforcement
has incorporated it. Resolve conflicts in the vault before restarting and retrying
writes; soft index exclusion is not a way around independent conflict guards.

`incompleteWrites` contains attempts with a start event and no terminal event,
not a list of all successful writes. There is no `list_recent_writes` MCP tool.
For each incomplete attempt, inspect the affected file/hash and relevant trash
path before choosing a new operation. The mutation may have happened even if
its response/audit completion was lost. No automatic rollback, replay, restore or
acknowledgement tool is provided. A recorded terminal failure is not included in
this list.

Audit rows store operation/path/hash metadata, not note content, and are not a
content backup. The live audit file is under `state_path`; configured retention
archives oversized stores at startup, but unfinished attempts defer rotation so
diagnostics remain available. Protect and back up the vault and state separately;
see [Configuration](CONFIGURATION.md).

## Optional OCR queue and page links

Enable `[ocr].enabled = true` and restart to register the three OCR tools. They
require `admin`. **This release implements a queue contract only:** there is no
OCR worker, notebook ingestion or page-renumbering implementation.

```sh
sb_call ocr_notebook '{"identifier":"demo-notebook","pages":[1,2],"force":false}'
sb_call ocr_renumber_notebook '{"notebook_id":"demo-notebook"}'
```

A queue response has `job_id`, `state: "queued"`, `type` and `queued_at`. To query
a job without hand-copying an ID:

```sh
SB_JOB=$(sb_call ocr_notebook '{"identifier":"status-demo"}' | jq -er '.result.structuredContent.job_id')
sb_call ocr_status "$(jq -nc --arg id "$SB_JOB" '{job_id:$id}')"
```

Status returns `id`, `type`, `state`, `queuedAt`, `updatedAt` and `input`. Jobs stay
queued and disappear on process restart; repeated submissions create new IDs.
`force` and `pages` are stored inputs, not evidence that work ran.

The always-available `link_to_page` tool is a separate indexed lookup under
`vault:read`. If an external workflow created a note with frontmatter
`source_id: rmpage:demo-notebook:page-1`, this call returns its wikilink:

```sh
sb_call link_to_page '{"notebook":"rmnotebook:demo-notebook","page_uuid":"page-1"}'
```

Without a visible indexed match it returns `link: null`. Queueing OCR does not
create that note or source ID.

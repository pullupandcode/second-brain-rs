# Configuration reference

[User guide](USER_GUIDE.md) · [Development example](../examples/config.development.toml) · [Production example](../config.example.toml)

Pass a TOML file with `second-brain-rs --config FILE`. The process reads it at
startup; changing it requires a restart. There is no general environment-variable
substitution, `.env` loader, command-line setting override, or TOML hot reload.
Unknown TOML keys are not rejected by the current parser, so a misspelled option
can be ignored: use the exact names below and verify the resulting behavior.

“Required” means the field/table must appear even when a feature is not being
used, including in development mode. Optional sections may be omitted. Start with
one of the complete example files rather than an isolated `[auth]` fragment.

## Listener and filesystem

| Key | Required/default | Meaning |
|---|---|---|
| `listen` | Required string | Socket address, e.g. `127.0.0.1:3000`. No TLS listener is built in. Development mode restricts its host to loopback. |
| `public_base_url` | Required HTTP(S) URL | Externally reachable resource origin, e.g. `https://brain.example.com`. Used in discovery and proxy Host validation. Prefer an origin without a path prefix. |
| `vault_path` | Required string | Existing Markdown vault root. The process needs read access and, for mutation workflows, write access. |
| `state_path` | Required string | Writable runtime directory, created at startup. Keep outside the vault and its sync rules. |

Use absolute filesystem paths for services. Relative filesystem paths, including
the config argument, resolve from the **working directory**, not from the TOML
file location. `~` and `~/...` are expanded only for `vault_path` and `state_path`,
using the `HOME` environment variable. Prefer explicit Windows paths instead of
relying on that variable being set.

```toml
# Windows literal strings avoid backslash escape sequences:
vault_path = 'C:\Notes\Vault'
state_path = 'C:\SecondBrain\State'
# Forward slashes also work, e.g. "C:/Notes/Vault".
```

Tool paths, skill map paths, trash paths and framework paths are **vault-relative**,
e.g. `Notes/Plan.md`, never `/srv/vault/Notes/Plan.md`. Tool paths normalize slashes
and dot segments; absolute paths, Windows drive prefixes and `..` traversal are
rejected. Paths are limited to 1,024 input bytes. Ordinary reads/writes expect
Markdown paths, and filesystem policy/symlink checks apply independently of scopes.

`public_base_url` does not mount the service under a URL prefix. Routes remain
`/mcp`, `/tools`, `/healthz`, and `/.well-known/oauth-protected-resource`. Use the
[deployment examples](DEPLOYMENT.md) for an origin-based reverse proxy.

## Authentication: `[auth]` is required

| Key | Required/default | Meaning |
|---|---|---|
| `mode` | `"jwt"` | `"jwt"` or `"development"`. No other modes are supported. |
| `audience` | Required nonempty string | Expected access-token audience; comparison is exact. Do not assume it must equal the resource URL. Match the provider's `aud`. |
| `trusted_issuers` | Required nonempty HTTP(S) URL array | Issuers allowed to sign tokens. URLs are parsed/normalized, then compared exactly to `iss`; trailing slash details matter. |
| `discovery_authorization_server` | Required HTTP(S) URL | Authorization server advertised to MCP clients. It does not configure an OAuth login flow or override the JWKS endpoint. |
| `jwks_cache_ttl_seconds` | Required nonnegative integer | Successful public-key cache lifetime; `0` disables cache reuse. |
| `jwt_algorithms` | `["RS256"]` | Nonempty allowlist containing only `RS256` and/or `ES256`. Select algorithms actually emitted by your provider. |
| `development_default_scopes` | `[]` | Fallback scope names in development mode when no known explicit scope is parsed. Ignored for JWT grants. Unknown names are discarded. |

The server derives the JWKS URL by resolving `.well-known/jwks.json` against each
configured issuer. It does not read OIDC discovery's `jwks_uri`, and there is no
`jwks_url` option. A key URL redirect is not followed. See
[OIDC compatibility and token requirements](AUTHENTICATION.md) before filling in
these values; the provider guides include exact endpoint mappings.

A valid signed access token still needs the tool's scope. Realm roles, groups,
`scp`, or an ID token's custom claim do not become permissions automatically.
Configure the access token's `scope` claim with the exact names required by
[the tool reference](TOOLS.md).

## Reading and indexing: `[index]` is required

| Key | Required/default | Meaning |
|---|---|---|
| `sqlite_path` | `{state_path}/index.sqlite` | Index database path. `":memory:"` makes only the index ephemeral; the audit store still uses state. |
| `watcher_polling` | Required Boolean | Compatibility option only. No native or polling background watcher is implemented. |
| `ignored_globs` | Required string array | Exclude matching paths from reader/list/index operations. Direct `read_note` is also affected. |
| `blocked_paths` | `[]` | Additional soft exclusions for direct reads, listings and index operations; not a write prohibition. |

The index is rebuilt at startup and after successful runtime mutations. External
filesystem edits do not automatically refresh it. Restart to perform a full
refresh; `framework_reload` and `skills_reload` have narrower responsibilities,
not a general configuration reload contract.

## Privacy: optional `[security]`

| Key | Default | Meaning |
|---|---|---|
| `blocked_paths` | `[]` | Hard deny patterns for ordinary vault reads, writes and framework access. Applies even to `admin`. |

Use `"Private/**"` for a subtree, not just `"Private"`. The latter is an exact
path rule. Hard deny matching normalizes Unicode NFC and ignores case; soft
index exclusions remain case-sensitive. This is a small glob subset, not a full
Gitignore implementation: exact paths, `Folder/**`, suffix patterns such as
`**/*.tmp`, and `*` runs are supported. Do not rely on negation (`!`), character
classes or brace expansion.

```toml
[index]
watcher_polling = false
ignored_globs = [".trash/**", "**/.DS_Store", "**/*.sync-conflict-*"]
blocked_paths = ["Archive/**"]

[security]
blocked_paths = ["Private/**", "Credentials.md"]
```

This hides `Archive/**` from ordinary reads/search/list but does not prohibit
writing there. It denies ordinary access to `Private/**` and `Credentials.md`.
Loaded skill maps and skill sources are automatically protected from ordinary
vault tools. Configured skills deliberately form a separate `skills:read` prompt
surface: configuring a blocked file as a skill can expose its prompt content to
that scope. Do not configure secrets as prompts.

## Mutations and deletion

| Table/key | Required/default | Meaning |
|---|---|---|
| `[writes].cooldown_seconds` | Required nonnegative integer | Existing-file mutation cooldown based on file modification time. `0` is useful in demos. |
| `[deletes].trash_path` | `.trash/mcp` | Soft-delete destination within the vault on the same filesystem. Normalized empty/root destinations are rejected. |

Existing-note mutations require the current note hash. Re-read and reconcile a
stale hash; do not retry blindly. The hash does not bypass cooldown or path policy.
Create/replace content publication is atomic within the supported filesystem
operations. Soft delete is a link/unlink sequence with an interruption window.
Unix replacement permission bits are preserved, but extended ACL/ownership
preservation is not guaranteed.

A custom trash directory needs a corresponding ignored pattern if it should not
appear in reads/search/listings:

```toml
[deletes]
trash_path = "Archive/Trash"
# Add "Archive/Trash/**" to your existing index.ignored_globs list.
```

## Audit and recovery: optional `[audit]`

| Key | Default | Meaning |
|---|---|---|
| `retention_max_rows` | `0` | At startup rotate when successful-write rows exceed this count. Zero disables rotation. |
| `archive_path` | `{state_path}/audit-archive` | Filesystem archive directory. Use the same filesystem as the live audit database. |

The live audit database is always `{state_path}/write-audit.sqlite`; `sqlite_path`
changes only the search index. Incomplete attempts defer rotation so diagnostics
remain queryable. The retention threshold is therefore soft and can be exceeded
until recovery is reconciled. There is no automatic archive deletion or incomplete
write replay. Back up both live state and archives; see [operations](DEPLOYMENT.md).

## Frameworks, daily notes and skills

| Table/key | Required/default | Meaning |
|---|---|---|
| `[framework].schema_path` | `_meta/framework.yaml` | Vault-relative base framework schema. Composition reads current file contents; use `framework_compose` to validate the result. |
| `[daily_note].capture_default_pattern` | Required `"A"` or `"B"` | Compatibility option. Current capture routing does not consult it; choose the desired tool explicitly. |
| `[skills].map_paths` | `[]` | Vault-relative Markdown maps from which prompt sources are discovered. Use `skills_reload` after content changes. |

`daily_note_get` uses `Calendar/Days/YYYY-MM-DD.md` and the existing template
`x/Templates/Daily Template.md`. Those paths and daily marker section names are
conventions in this implementation, not configurable TOML fields. The framework
schema controls record types and templates; it does not override those daily paths.
Framework field `required`/default declarations are descriptive rather than a
full validation/defaulting engine. See [daily and record examples](USE_CASES.md).

## Optional OCR and required logging

| Table/key | Required/default | Meaning |
|---|---|---|
| `[ocr].enabled` | `false` if section omitted; required Boolean if `[ocr]` is present | Advertise three in-memory OCR queue/status tools. No OCR worker is supplied. |
| `[logging].log_args` | Required Boolean | Include arguments after credential-field redaction when `true`. Keep `false` to log hashes instead. |

Argument logging can expose note bodies and arbitrary private fields even when
known credentials are redacted. It is not a general personal-data filter. JSON
logs are emitted to the process output; your service manager controls collection
and retention. Authentication logs record outcome/subject/issuer and the directly
connected peer, not a trusted client IP inferred from forwarding headers.

## Environment and restart reference

| Variable | Effect |
|---|---|
| `RUST_LOG` | Tracing filter; defaults to `info` if no valid filter is supplied. |
| `HOME` | Used for the limited `~` expansion described above. |
| `SECOND_BRAIN_ALLOW_DEV_AUTH=1` | Bypasses the loopback restriction in development mode. It does not add authentication; avoid it on shared/untrusted networks. |

The `SB_TOKEN` and `SB_MCP_URL` variables used in these guides belong to the
**example client shell**. They are not server configuration options.

| Change | Apply it with |
|---|---|
| TOML auth/listener/policy/scopes/path settings | Restart the server |
| Skill map/source contents at existing configured paths | `skills_reload` with `admin` |
| Framework base/overlay file contents | Read on composition; use `framework_compose` to validate the base and overlays, and `framework_reload` for per-overlay parse diagnostics |
| External notes affecting search, or sync conflicts affecting quarantine | Restart for a complete refresh |
| Identity-provider public keys | Publish before token rollover; allow cache/refresh behavior described in [authentication](AUTHENTICATION.md) |

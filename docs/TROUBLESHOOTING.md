# Troubleshooting

[User guide](USER_GUIDE.md) · [Configuration](CONFIGURATION.md) · [Authentication](AUTHENTICATION.md)

## Start with a reproducible request

Check the process log, public `/healthz`, discovery metadata, and a scoped
`read_note` against a disposable known file. Health and MCP initialization do not
prove authorization. Record the server release, request method/tool, error code,
and whether the failure occurs directly on loopback or through the proxy.
Do not include Bearer tokens, client secrets or private note bodies in reports.

Use [the request helpers](DEVELOPMENT.md#reusable-bash-request-helpers) to separate
server behavior from client login/configuration problems. Inspect `.error` in the
JSON-RPC response even when HTTP is 200. Successful tool payloads are under
`.result.structuredContent`; arrays can be wrapped in its `result` field.

## Startup and installation

| Symptom | What to check |
|---|---|
| Usage error | Supply `--config /absolute/path/config.toml`. There is no implemented `--help` or `--version`; initialization reports the version. |
| Configuration fails to load | Start from a complete example. Required tables/fields remain required in development mode. Check TOML quoting, nonempty audience/issuer list, valid URLs and algorithms. |
| A setting has no effect | Unknown TOML keys can be ignored. Compare spelling and table placement with the [full reference](CONFIGURATION.md). Configuration is read only at startup. |
| Files appear in the wrong directory | Relative filesystem paths resolve from the process working directory, not the config file directory. Use absolute service paths. Environment variable interpolation is not supported. |
| Windows path parse error | Use a TOML literal string such as `'C:\SecondBrain\vault'`, or forward slashes. Backslashes in double-quoted TOML strings are escapes. |
| Permission denied / runtime cannot initialize | Create the vault first; grant the service account access to vault, state, SQLite and archive paths. Verify its working directory and permissions, including restored files. |
| Address already in use | Stop the other process or choose an unused `listen` port and update `public_base_url`/proxy accordingly. |
| Development bind refused | Use loopback. The non-loopback override exists for isolated development only; use JWT mode for a network deployment. |
| Linux binary cannot load glibc / wrong executable format | Choose the correct target. Supplied Linux binaries use GNU/glibc and Ubuntu 24.04 builds; build from source for an incompatible runtime/architecture. |
| macOS download is blocked | Binaries are not Developer-ID signed/notarized. Verify the release/checksum and follow your organization's per-application approval policy. |

## Authentication, scopes and clients

| Symptom | What to check |
|---|---|
| JWT rejected | Use the **access token**, not an ID token, refresh token or opaque token. Check signed `iss`, `aud`, `sub`, `exp`, algorithm and key ID locally. See [token requirements](AUTHENTICATION.md). |
| Issuer looks right but fails | Matching is exact after Rust URL serialization of the configured issuer. Root URLs gain a trailing `/`; provider signed claims may not. Authelia needs the specific [subpath recipe](oidc/authelia.md). |
| Provider discovery works but authentication fails | This release does not follow discovery `jwks_uri`. Test the **derived JWKS URL** and provider alias from its guide; it must return a usable JSON key set without a redirect. |
| Key rollover causes failures | Ensure the new public key is published and its metadata matches the token algorithm/key ID. Refresh failures are rate-limited; allow the cooldown and retry after fixing the endpoint. Do not repeatedly rotate keys as a workaround. |
| Token expired/not yet valid | Check system clocks and token lifetime. Obtain a new access token. Expiry is required. |
| Health/initialize works but calls fail | Protected operations authenticate independently on every request. Send Authorization every time; initialization/session headers do not establish an authenticated session. |
| Tools missing or insufficient scope | Scopes must be issued in supported token claims. Roles/groups alone do not grant access. `admin` does not imply read/write, and `skills:read` exposes prompts rather than tools. |
| Development token seems ignored | Use exact `Authorization: Bearer scope=vault:read`. Missing/malformed/unknown-only claims use fallback scopes; recognized explicit scopes replace the fallback set. Development scope tokens never work in JWT mode. |
| Client login cannot register itself | The server supplies resource metadata, not a token endpoint or client registration service. Pre-register the client at the provider and use its actual callback URI. Client OAuth support varies. |
| Browser UI cannot fetch MCP | The project supplies an HTTP MCP server, not a browser UI or a documented cross-origin browser integration. Use a compatible MCP client; do not assume proxy HTML login is API authentication. |

Do not paste live tokens into online JWT decoders. The provider guides show local
inspection; decoding is diagnostic only and does not verify a signature.

## HTTP and reverse proxy

| Symptom | What to check |
|---|---|
| Host rejected | Forward the original Host and configure the externally visible origin in `public_base_url`, including any nonstandard port. |
| 404 at `/brain/mcp` | Routes are mounted at the origin root. `public_base_url` does not create a path prefix. Publish `/mcp`. |
| `/docs` is missing | Discovery advertises a documentation URL, but the binary does not serve a `/docs` route. Read this repository's guide or serve docs at your proxy. |
| MCP media-type/protocol error | Send JSON-RPC with Content-Type `application/json` and Accept `application/json, text/event-stream`. Optional `mcp-method`/`mcp-name` headers must agree with the body. |
| Response is HTML / redirect | Bypass proxy interactive-login interception for the API and send a provider-issued Bearer token. JWKS also must return JSON directly. |
| Large request fails | The MCP HTTP request-body limit is 1,000,000 bytes. Split the operation into smaller note changes. |

## Notes, writes and stale results

| Symptom | What to check |
|---|---|
| `path_blocked` / missing private file | Check `security.blocked_paths`, soft index exclusions, skill map/candidate privacy and symlink targets. `admin` does not bypass privacy. Do not move secrets into a public folder to make a call succeed. |
| `path_quarantined` | Inspect `list_vault_conflicts` with `admin`, resolve the sync conflict outside the server, and restart to rebuild the quarantine snapshot. |
| `path_exists` on create | Creation does not overwrite an existing note. Read it and intentionally replace with its current hash, or choose another path. |
| `path_missing` | Use a vault-relative path, correct case and existing file. Tool paths are not operating-system absolute paths. |
| `retryable_conflict` on replacement | Reread the note and compare changes before retrying with the new `currentSha256`; also wait for the configured write cooldown. Do not retry blindly over another editor's work. |
| Search/backlinks miss an external edit | Direct reads are fresh; the index has no background watcher. Restart to rebuild it. Backlink matching uses the stored link target, which can differ from a full note path. |
| Files hidden from reads can still be written | `index.ignored_globs`/`index.blocked_paths` are not write prohibitions. Use `security.blocked_paths` for ordinary read/write denial. |
| Delete succeeded but file appears in trash | `delete_note` is soft deletion. `hard_delete_note` is permanent and needs its separate scope and current hash. |
| Incomplete write diagnostics | Compare the source/trash/target and current hash before a manual retry. Audit starts are best effort, so an empty pending list is not proof that no interrupted filesystem operation occurred. |
| State directory grows | Check audit retention, archives and logs. Rotation is startup-only and may defer pending attempts; archives are not automatically deleted. Back up before manual maintenance. |

## Frameworks, daily notes and prompts

| Symptom | What to check |
|---|---|
| Record type / vault structure unavailable | Initialize a framework or supply `_meta/framework.yaml` (or configured schema path). See [framework workflows](USE_CASES.md). |
| Record fields/defaults are not enforced | Framework declarations are descriptive in this release. Required-field enforcement and automatic default application are not implemented. |
| Capture does not append to a daily note | Capture tools create records. `daily_note.capture_default_pattern` does not route them. Use `daily_note_append` explicitly. |
| Missing daily template | Create `x/Templates/Daily Template.md` with the exact paired markers shown in [daily workflows](USE_CASES.md). Daily paths are `Calendar/Days/YYYY-MM-DD.md`. |
| Marker append rejected | Use the supported writable section (`daily-log` or `last-light`) with one opening/closing marker pair. Agenda is readable but not appendable. Inspect before using admin repair. |
| A read unexpectedly creates a daily file | `daily_note_get` creates a missing daily note under `vault:read`; use `read_note` for a strictly non-creating read. |
| Skill prompt not listed | Check configured map paths, links, lowercase slug name, description and nonempty body. Run `skills_list`/`skills_reload` with `admin`; list/get prompts with `skills:read`. |
| Prompt text is private to ordinary reads | This is intended. Skill notes/maps and candidates are blocked from ordinary vault tools. Treat `skills:read` as a separate privilege that can disclose configured prompt content. |
| OCR job never produces text | OCR tools only maintain an in-memory queue/status contract. No OCR worker runs and jobs do not survive restart. |

## Logs and support reports

Set `RUST_LOG=debug` for a focused local reproduction if info logs are insufficient;
restart to apply the environment. Keep `logging.log_args = false` for normal use.
Enabling it can record note content even though conventional credential fields
are redacted. Preserve audit data when investigating interrupted writes.

A useful issue includes release/OS, a minimal sanitized config, tool name and
arguments with private content removed, HTTP status and JSON-RPC error, relevant
sanitized log lines, and whether restart changes the result. Use a disposable
vault to reproduce destructive or concurrency problems.

# User guide

second-brain-rs connects a Markdown vault to an assistant or automation through
Streamable HTTP MCP. It reads and searches notes, performs guarded writes,
creates framework records and captures, manages daily notes, and serves private
skills as MCP prompts. It is a server, not a chat UI or an identity provider.

These instructions describe **v0.5.2**, reviewed against the implementation on
2026-09-21. Start with a disposable vault before connecting an existing collection.

## Choose your path

| Goal | Read |
|---|---|
| Install a binary and make a first connection | [Getting started](GETTING_STARTED.md) |
| Try the server without an identity provider | [Local development mode](DEVELOPMENT.md) |
| Look up every TOML option, default and path rule | [Configuration reference](CONFIGURATION.md) |
| Search, write, capture, use daily notes and skills | [Common use cases](USE_CASES.md) |
| Look up tool arguments and scope requirements | [Tool reference](TOOLS.md) |
| Connect an authenticated client | [Authentication and OIDC](AUTHENTICATION.md) |
| Configure an identity provider | [Authentik](oidc/authentik.md), [Authelia](oidc/authelia.md), [Keycloak](oidc/keycloak.md) |
| Run a persistent HTTPS service and manage backups | [Deployment and operations](DEPLOYMENT.md) |
| Diagnose startup, auth or tool failures | [Troubleshooting](TROUBLESHOOTING.md) |
| Understand binary builds and release checksums | [Release operations](RELEASING.md) |

For production, follow installation → provider compatibility → JWT configuration
→ HTTPS deployment → a scoped read test. An HTTP health check or MCP initialization
alone does not verify authentication or access to the vault.

## What lives where

- **Executable:** an extracted release binary, or a build from this repository.
- **Configuration:** the TOML file passed with `--config`; it is read at startup.
- **Vault:** your existing Markdown directory, with paths expressed relative to
  its root in tool calls. New folders may be created by writes.
- **State:** a separate writable directory for the index, write audit and archives.
  It is not a second copy of the vault and must not be synchronized into the vault.
- **MCP client:** connects to `/mcp` and sends a Bearer header on each request.
  The client obtains and refreshes production access tokens from your provider.

There is no built-in browser UI, stdio transport, OAuth authorization/token
endpoint, automatic client registration, or login callback handler. The `/docs`
URL advertised in discovery is not a built-in documentation route; serve a guide
there through your reverse proxy if your clients follow that link.

## Behavior to understand before granting access

Scopes are independent. `admin` is not an all-access scope; grant each permission
needed for a workflow. `daily_note_get` is a deliberate exception to read-only
semantics: its `vault:read` scope can create a missing daily note from a template.
Use filesystem permissions as an additional boundary when writes must be impossible.

The index rebuilds at startup and after successful server mutations. External
edits are visible to direct reads but may leave search results stale until a
restart or rebuild caused by another mutation. There is no background watcher.
Conflict quarantine is captured at startup; restart after resolving or creating
external sync-conflict files to update write enforcement.

A mutation may have completed even if a client loses its connection. Read the
note and inspect recovery diagnostics before retrying. Soft deletion uses a
trash hard link followed by unlinking the original; an interruption can leave
both names. Audit logging is best effort and cannot guarantee a recovery record
when recording the start itself fails. See [recovery](USE_CASES.md) and
[operational backups](DEPLOYMENT.md).

The three optional OCR tools implement an in-memory queue/status contract only.
They do not convert PDFs, recognize handwriting, or run an OCR worker.

Developer contributions and CI instructions are separate in
[CONTRIBUTING.md](../CONTRIBUTING.md). The historical parity delivery records are
in [PARITY_STATUS.md](PARITY_STATUS.md); they are not setup prerequisites.

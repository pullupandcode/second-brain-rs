# Deployment

Run the server as an unprivileged service account with access limited to its
vault, state directory, and configured archive directory. Use a separate state
path for each running instance. Back up the vault and audit state. The server
coordinates operations within one process; avoid multiple writer processes
against the same vault/state files.

Terminate HTTPS at a trusted reverse proxy and bind the Rust process to loopback
or a private interface. Configure `public_base_url` to the externally reachable
HTTPS origin and preserve the configured public Host. Publish `/mcp`,
`/.well-known/oauth-protected-resource`, and optionally `/healthz` and `/tools`.
Forward Authorization headers unchanged. Do not inject development scope tokens
at the proxy. Never expose development auth to untrusted callers.

Example Caddy configuration for a server listening on `127.0.0.1:3000`:

```caddyfile
brain.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

Set `public_base_url = "https://brain.example.com"`. Allow outbound HTTPS to each
configured issuer's JWKS endpoint. Redirects are not followed; configure the
actual key endpoint through the issuer URL layout. Scope issuance and user login
remain the identity provider's responsibility. See [authentication](AUTHENTICATION.md).

The HTTP surface emits security headers and disables response caching. It uses
stateless JSON MCP responses; identity and scopes are checked independently for every dispatched operation.
Protocol-only preflight responses do not access vault state and precede auth.
Do not rely on a previous initialization request or session header to retain
permission after token scope changes.

| Client integration | Configuration |
|---|---|
| Streamable HTTP MCP client | Public `/mcp` endpoint; Bearer JWT on each request; accept JSON and event-stream media types |
| OAuth-capable MCP client | Discover the protected resource metadata; obtain a matching audience and required scopes from the advertised authorization server |
| Direct HTTP inspection | Bearer JWT on `GET /tools`; public `GET /healthz` and resource metadata |
| Local test client | Loopback listener, explicit development mode, limited fallback scopes |

Client-specific OAuth setup differs. The server supplies resource metadata and
verifies tokens; it does not promise compatibility with every client's login or
dynamic registration workflow. Identity providers must supply usable JWT access
tokens and an accessible public key set.

Grant scopes by workflow. Read clients need `vault:read`; prompt clients also
need `skills:read`. As in the reference, `daily_note_get` may create a missing
daily note under `vault:read`; do not promise strictly read-only filesystem access
from that scope alone. Captures and daily appends can be granted without unrestricted
write access. Keep `admin`, soft delete, and permanent delete separate. Filesystem
privacy policy applies even to privileged callers.

Authentication logs identify the directly connected peer; behind a proxy this
is normally the proxy's address. Forwarded client addresses are not trusted as
authorization identity. Protect logs and audit archives, and keep `log_args`
disabled unless sensitive content logging is explicitly intended.

Upgrade using the immutable GitHub release matching the reviewed version. Keep
configuration and state outside the checkout, back up before upgrading, stop the
old process, install the new binary, start it, and verify health, discovery, a
scoped read, and audit diagnostics. Refer to `CHANGELOG.md` for migration notes
and `docs/PARITY_STATUS.md` for the review and release ledger.

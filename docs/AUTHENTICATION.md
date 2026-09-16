# Authentication and authorization

Production deployments use `[auth] mode = "jwt"`. The server verifies access
tokens; it does not issue tokens, implement login, or turn an ID token into an
access token. Configure an OAuth authorization server that can issue JWT access
tokens for this resource and publish the corresponding public JWKS.

```toml
[auth]
mode = "jwt"
audience = "second-brain-rs"
trusted_issuers = ["https://identity.example/application/o/brain/"]
discovery_authorization_server = "https://identity.example/application/o/brain/"
jwks_cache_ttl_seconds = 3600
jwt_algorithms = ["RS256", "ES256"]
```

The token `iss` must exactly equal one configured issuer URL, including its path
and trailing slash. `aud` must contain the configured audience. A nonempty
string `sub` and numeric `exp` are required. Expired tokens are rejected at the
expiration second; optional `nbf` is enforced with no clock-skew allowance.
Synchronize the server and issuer clocks. Only the explicitly configured RS256
and ES256 algorithms are accepted. Symmetric algorithms, token-provided key
URLs, unknown critical headers, incompatible JWK metadata, and ambiguous key
matches are rejected. `jti` and `client_id` are optional identity metadata.

The server resolves `.well-known/jwks.json` relative to each configured issuer,
just as a URL relative reference. For an issuer ending in `/application/o/brain/`,
the endpoint is `/application/o/brain/.well-known/jwks.json`. Configure trailing
slashes deliberately. Requests use certificate validation, TLS 1.2 or newer,
no redirects, a five-second deadline, and a 1 MiB response limit. HTTP issuers
are supported for local tests; use HTTPS in deployment.

Keys are cached independently for each trusted issuer. Concurrent refreshes
share one request. A missing key ID can trigger a refresh after a 30-second
cooldown; a normal TTL expiry refreshes cached keys. Failed fetches are also
throttled for 30 seconds. Expired cached keys are never used after fetch failure.
A still-fresh known key remains usable during a failed unknown-key refresh.
Publish new keys before minting tokens that use them and keep old keys until
previously issued tokens expire. Tokens are limited to 16 KiB.

`GET /.well-known/oauth-protected-resource` provides the public resource URL,
authorization server, and supported scopes. `/healthz` and discovery are public.
`/tools` and each dispatched MCP operation authenticate independently.
Protocol preflight can answer ping, empty resource listings, notifications,
and malformed requests before authentication; it never dispatches vault tools. Missing or invalid
production tokens return HTTP 401 with `WWW-Authenticate`; errors do not contain
token contents, cryptographic diagnostics, upstream bodies, or filesystem paths.
Authorization denial for an authenticated MCP tool call is `forbidden_scope`.

Scopes are independent; `admin` does not imply vault read or write access.
The `scope` claim accepts a whitespace-separated string or an array of scope
strings. Unknown scopes are discarded; missing or malformed scope claims grant
no scopes. Mixed-type arrays grant no scopes. The eight supported scopes are:

| Scope | Permission |
|---|---|
| `vault:read` | Read, search, list, links, daily get/create, structure, record types, source-page links |
| `skills:read` | List and retrieve MCP prompts from configured skill maps |
| `vault:write` | Create, replace, frontmatter, marker writes, records |
| `vault:delete` | Soft-delete notes |
| `vault:delete:hard` | Permanently delete notes |
| `vault:capture` | Inbox captures |
| `daily:append` | Append daily notes |
| `admin` | Framework, skill reload, conflict listing, marker repair, audit recovery, and enabled OCR job tools |

`daily_note_get` is the reference's exception to read-only behavior: it can create
a missing daily note from its configured conventional template while using
`vault:read`. It still enforces filesystem privacy and audited writing.

## Local development

Development mode accepts `Authorization: Bearer scope=vault:read` and never
verifies a signature. If no known explicit scope is present—including a missing
or malformed header—it grants `development_default_scopes`, matching the pinned
TypeScript implementation. An empty fallback grants nothing. This corrects the
earlier Rust skeleton's missing-header 401 behavior.

Use a loopback listen address. Non-loopback development auth is rejected unless
an operator explicitly sets `SECOND_BRAIN_ALLOW_DEV_AUTH=1`. That escape hatch
is for isolated test environments: development credentials let callers choose
any known scope and cannot protect a public server.

## Logs and reference differences

Authentication logs contain outcome, validated subject and issuer on success,
error code on denial, and the directly connected peer IP when available. Behind
a proxy, the peer is the proxy; untrusted forwarding headers are not treated as
client identity. Token contents and token IDs are not logged.

Operational logs normally contain argument hashes. `[logging] log_args = true`
additionally includes arguments after conventional credential-key and Bearer-value
redaction. This remains sensitive: note text and arbitrary user-defined fields
may contain private data. Leave it false in production unless your retention and
access policies explicitly permit those contents. Redaction is not a general
PII detector.

Parity targets `second-brain-mcp` commit
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`. Required expiry is an intentional
hardening from adopted issue #38: the reference's jose verifier checks `exp` when
present but does not require it. Persistent caches, bounded fetches, guarded
rollover, and sanitized failures are deliberate safety improvements. Verification
uses jsonwebtoken with AWS-LC, avoiding the RustCrypto RSA dependency affected by
RUSTSEC-2023-0071. See the scope report for test and mutation evidence.

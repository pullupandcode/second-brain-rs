# Authentication and authorization

This guide describes second-brain-rs **v0.5.2**. Start with the
[local setup](GETTING_STARTED.md), then use [Authentik](oidc/authentik.md),
[Authelia](oidc/authelia.md), or [Keycloak](oidc/keycloak.md) for production.
The provider guides were checked against official documentation on 2026-09-21;
they are configuration recipes, not reports of live provider integration tests.

## What connects to what

second-brain-rs is an OAuth **resource server**: it validates a bearer access
token before serving vault tools. Your MCP application is the OAuth client;
your identity provider handles login, consent, token issuance, and refresh.
The server has no login page, OAuth callback, token endpoint, client secret,
client registration service, or token refresh endpoint. It does not introspect
opaque tokens or consult a provider's revocation endpoint.

Register the **MCP application's callback URL** at your provider, not `/mcp` on
second-brain-rs. Desktop/browser applications should use a public client and
authorization code with PKCE S256. A confidential backend may use a client secret
at the provider; that secret never belongs in second-brain-rs configuration.
Configure the MCP client with its registered client ID, callback, provider
metadata, and requested scopes. An MCP client must support your provider's
registration/login arrangement; resource discovery alone does not make every
client compatible.

Send the token endpoint response's **`access_token`**, not `id_token` or
`refresh_token`, in `Authorization: Bearer …`. It must be a signed JWT with the
claims below. This verifier does not enforce a token-type discriminator such
as `typ=at+jwt`; correct provider audience/scope separation and correct client
token selection therefore matter. Opaque and encrypted access tokens cannot be
used. Provider logout does not invalidate an already issued JWT here; use
appropriate access-token lifetimes.

## Configure JWT verification

Merge this section into your complete [configuration](CONFIGURATION.md):

```toml
[auth]
mode = "jwt"
audience = "second-brain-rs"
trusted_issuers = ["https://auth.example.com/application/o/brain/"]
discovery_authorization_server = "https://auth.example.com/application/o/brain/"
jwks_cache_ttl_seconds = 3600
jwt_algorithms = ["RS256"]
development_default_scopes = []
```

`audience` is the exact resource audience issued in the access token, not
necessarily the server URL or the OAuth client ID. Each provider guide supplies
a matching value. `trusted_issuers` controls verification;
`discovery_authorization_server` only tells clients where to obtain tokens.
Changing discovery does not change trust or the JWKS address. Configure
`public_base_url` separately as your externally reachable server URL.

| JWT field | Requirement |
|---|---|
| Protected `alg` | Enabled in `jwt_algorithms`; only `RS256` and `ES256` exist. ES256 requires a P-256 key. Default algorithm is RS256. |
| Protected `kid` | Use the ID of a compatible public signing key in JWKS. It can be absent only when exactly one compatible key can be selected. |
| `iss` | String exactly matching a configured issuer **after URL normalization**, including its path and trailing slash. |
| `aud` | Expected audience string, or an array containing that string. |
| `sub` | Nonempty string identifying the subject. |
| `exp` | Required number of epoch seconds, strictly later than the current whole epoch second. |
| `nbf`, `iat` | Optional numbers. `nbf` must not be in the future; `iat` is type checked, not a maximum-age rule. |
| `scope` | Space-separated string of permissions, or an array consisting entirely of scope strings. |

Synchronize clocks: there is no clock-skew allowance. Fractional `exp` and `nbf`
are compared without rounding the claim. `jti` and `client_id` are optional
metadata, not authorization rules. Groups, realm roles, email addresses, and
`scp` are not substitutes for `scope`.

## JWKS routing: a required provider adaptation

v0.5.2 does **not** discover keys through OIDC metadata or its `jwks_uri`, and
there is **no `jwks_url` configuration option**. The server computes
`issuer_url.join(".well-known/jwks.json")`. A path without a final slash loses
its last segment during that relative resolution:

| Configured issuer | URL fetched by second-brain-rs |
|---|---|
| `https://auth.example.com/application/o/brain/` | `https://auth.example.com/application/o/brain/.well-known/jwks.json` |
| `https://sso.example.com/realms/brain` | `https://sso.example.com/realms/.well-known/jwks.json` |
| `https://auth.example.com/authelia` | `https://auth.example.com/.well-known/jwks.json` |

The provider guides supply an exact reverse-proxy alias from that computed URL
to the provider's native public JWKS endpoint. Put it on the **identity
provider's hostname**, preserve the signed issuer, and serve the JSON directly.
An HTTP redirect will fail. The aliases use Nginx's URI replacement in
[`proxy_pass`](https://nginx.org/en/docs/http/ngx_http_proxy_module.html#proxy_pass);
the surrounding HTTPS virtual host and normal provider proxy remain yours.

**Origin-only issuer limitation:** Rust normalizes `https://auth.example.com`
to `https://auth.example.com/`. A provider that signs the former, without a
slash, cannot authenticate directly in v0.5.2. Typing the no-slash value into
TOML does not avoid normalization. A JWKS alias cannot fix this signed-claim
mismatch. The [Authelia guide](oidc/authelia.md) explains its root-hosted blocker
and a separately configured subpath deployment.

JWKS retrieval verifies TLS certificates, requires TLS 1.2 or newer for HTTPS,
rejects redirects, times out after five seconds, and caps the response at 1 MiB.
HTTP issuers exist for local tests; deploy with HTTPS. Keys are held in memory
per issuer for `jwks_cache_ttl_seconds`. An unknown key can trigger refresh, with
a 30-second guard against repeated misses/failures. Expired cached keys fail
closed when refreshing fails; a still-fresh known key remains usable after an
unknown-key refresh failure. Publish replacement keys before using them and
retain old keys until their issued tokens expire. A restart empties the cache.
Tokens are limited to 16 KiB. Token-supplied key URLs are ignored.

## Grant permissions explicitly

Start with `vault:read`. Add only the scopes needed by your client and enforce
who may obtain them **at the provider**. Every successfully authenticated subject
uses the same configured vault; there are no per-user vaults or user-specific
path permissions. The [filesystem denylist](CONFIGURATION.md) applies to all
subjects. Advertising a scope in discovery does not grant it.

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

Scopes are independent: `admin` grants neither read nor write access. Unknown
names are discarded; a missing or malformed claim grants nothing. Mixed-type
arrays grant nothing; array elements must match exactly. String scopes use
ECMAScript whitespace, including U+FEFF and excluding U+0085; ordinary ASCII
spaces are the interoperable choice.

`daily_note_get` can **create a missing daily note** with `vault:read`, using its
conventional template. It still enforces filesystem privacy and audited writes.
Use [tool documentation](TOOLS.md) when deciding what a permission allows.

## Obtain and check an access token

1. Complete the chosen provider guide, including its JWKS alias.
2. Configure your OAuth-capable MCP client with the guide's client ID and
   provider metadata URL. Register that client's exact callback URL. Request
   `openid vault:read` initially; the `openid` scope itself grants no vault access.
3. Let the client generate its PKCE verifier/challenge and state, open the
   authorization request, validate the callback state, and exchange the code
   using its original verifier. For manual diagnostics, use an OAuth client
   capable of this flow; second-brain-rs cannot generate a token for you.
4. Use the returned access token with the remote MCP URL
   `https://brain.example.com/mcp`. Let the client renew it through the provider.

A bearer-only MCP client needs an access token obtained elsewhere and must have
its token replaced when it expires. There is no universal client configuration
format; match the client's supported HTTP bearer/OAuth settings.

This Python 3 check prompts without echoing the token or placing it in shell
history. Replace the server URL. It makes one authenticated, non-mutating
`GET /tools` request and prints the accessible tool names:

```python
import getpass
import json
import urllib.error
import urllib.request

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

opener = urllib.request.build_opener(NoRedirect)
base = "https://brain.example.com"
token = getpass.getpass("JWT access token: ")
request = urllib.request.Request(
    base + "/tools", headers={"Authorization": "Bearer " + token}
)
try:
    with opener.open(request, timeout=10) as response:
        tools = json.load(response)["tools"]
        print("Authentication accepted. Visible tools:")
        print("\n".join(tool["name"] for tool in tools) or "(none)")
except urllib.error.HTTPError as error:
    print("HTTP", error.code)
    print("WWW-Authenticate:", error.headers.get("WWW-Authenticate", ""))
```

An HTTP 200 with no visible tools means the token authenticated but carries no
applicable tool scopes. Do not upload a live token to a public JWT decoder.
Locally decoding a JWT is useful for inspecting fields, but is not signature
verification; the request above exercises server verification.

## Troubleshooting

| Symptom | Check |
|---|---|
| HTTP 401 | Access token, exact normalized `iss`, audience, expiry, `sub`, algorithm, and accessible matching JWKS key. |
| Native JWKS works, server still returns 401 | Fetch the **computed alias URL** without following redirects; confirm HTTP 200 and a JSON `keys` array. Discovery's `jwks_uri` is not used. |
| Authelia root issuer rejected | Its no-slash origin does not match Rust's normalized issuer; follow the provider guide's compatibility note. |
| Empty tool list / `forbidden_scope` | Inspect the access token's `scope`; ID-token attributes, roles, and groups do not grant these permissions. |
| Rotation causes intermittent failures | Check the 30-second refresh guard, JWKS publication timing, cache TTL, and old-key retention. |
| Login succeeds but MCP cannot log in | Check client registration/callback/PKCE support and whether the client obtains JWT access tokens for the configured audience. |
| HTTP 403 before token validation | Check proxy Host handling and `public_base_url`; see [deployment](DEPLOYMENT.md). |

`/healthz` and `/.well-known/oauth-protected-resource` are public. Resource
metadata advertises the configured authorization server and scopes, but its
`resource_documentation` points at `/docs`, which the binary does not serve.
Initialization, ping, empty resource listings, notifications, and malformed
protocol preflight may be answered without authentication; no vault tool is
dispatched by those responses. Authenticate every actual MCP operation.

## Local development and logging

`mode = "development"` accepts `Authorization: Bearer scope=vault:read` without
signature verification. Missing/malformed headers or tokens containing no known
scope receive `development_default_scopes`; an empty fallback grants nothing.
Any caller can explicitly choose any known scope, including `admin`.
Use a loopback listener. Non-loopback development auth is rejected unless
`SECOND_BRAIN_ALLOW_DEV_AUTH=1` is set for an isolated test environment.
See [development mode](DEVELOPMENT.md) for a complete local configuration.

Authentication logs record outcome, validated subject/issuer on success, error
code on denial, and direct peer IP when available. Behind a proxy that is the
proxy's IP. Tokens and token IDs are not logged. `[logging] log_args = false`
logs argument hashes; enabling it also logs arguments with conventional
credential/Bearer redaction. Note bodies and arbitrary fields can still expose
private information. It is not a general PII detector.

Required `exp` is intentional hardening beyond the pinned TypeScript reference,
which validates expiry only when present. See [parity evidence](parity/auth.md)
for test history. These provider instructions do not add deployment-test evidence.

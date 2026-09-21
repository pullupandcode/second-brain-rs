# Keycloak

Use this guide with second-brain-rs **v0.5.2** and a Keycloak installation matching
the current administration/protocol-mapper documentation checked on
**2026-09-21**. It assumes a functioning HTTPS realm at
`https://sso.example.com/realms/brain`. These instructions have been checked
against docs and source behavior, **not a live Keycloak deployment**; installed
versions may have different menu labels. Read the
[authentication model](../AUTHENTICATION.md) first.

## Register the OAuth client and scope

In the `brain` realm:

1. Create an OpenID Connect client named `second-brain-client`. Disable **Client
   authentication** (public client); enable **Standard flow**. Disable implicit
   flow and direct access grants. Register the MCP application's exact callback,
   for example `http://127.0.0.1:8765/callback` only when it actually uses that
   address. Do not register the server's `/mcp` URL as a callback.
2. In advanced settings require PKCE **S256**. Keep normal JWT access tokens;
   do not enable lightweight tokens for this recipe. Preserve the default
   `basic` client scope, which supplies `sub`.
3. Create an OpenID Connect client scope named **`vault:read`**. Enable
   **Include in token scope**, then attach it to `second-brain-client` as
   **Optional**. Request `openid vault:read` during login.
4. Confirm the realm has an active RSA signing key and uses **RS256** for this
   client's access tokens.

These settings are covered by the
[Keycloak administration guide](https://www.keycloak.org/docs/latest/server_admin/).
A role named `vault:read` alone is insufficient: second-brain-rs reads the token's
`scope`, not `realm_access` or `resource_access`. Use the exact scope name above.

## Add the resource audience to access tokens

Open the client's dedicated client scope (typically
`second-brain-client-dedicated`), then **Mappers → Configure a new mapper →
Audience**. Set:

| Setting | Value |
|---|---|
| Name | `second-brain-audience` |
| Included Client Audience | Leave empty |
| Included Custom Audience | `second-brain-rs` |
| Add to access token | On |
| Add to ID token | Off |

For administrators using the REST API, the equivalent mapper representation is:

```json
{
  "name": "second-brain-audience",
  "protocol": "openid-connect",
  "protocolMapper": "oidc-audience-mapper",
  "config": {
    "included.custom.audience": "second-brain-rs",
    "access.token.claim": "true",
    "id.token.claim": "false",
    "lightweight.claim": "false"
  }
}
```

Attach it to that client's dedicated scope; the JSON is a mapper object, not a
whole realm import. This adds the expected resource to the access token's `aud`
without replacing existing audiences. Do not leave a competing Included Client
Audience selected: it takes precedence over the custom audience.
See [Audience mapper fields](https://www.keycloak.org/admin-api/protocol-mappers).

The OAuth client ID remains `second-brain-client`; the API audience is
`second-brain-rs`. These are deliberately distinct. Changing only the server's
`audience` setting cannot cause Keycloak to issue a new audience.

## Add the JWKS alias without changing the issuer

Keycloak publishes realm keys at
`/realms/brain/protocol/openid-connect/certs` and realm metadata at
`/realms/brain/.well-known/openid-configuration`. Its issuer normally has **no
trailing slash**. See [OIDC endpoints](https://www.keycloak.org/securing-apps/oidc-layers).

v0.5.2 instead joins `.well-known/jwks.json` to the configured issuer. The result
is **`/realms/.well-known/jwks.json`**, because `brain` is the final path segment,
not a slash-terminated directory. Appending a slash to the configured issuer
would break exact JWT issuer matching.

In the existing HTTPS Nginx virtual host for **sso.example.com**, add:

```nginx
location = /realms/.well-known/jwks.json {
    proxy_pass http://127.0.0.1:8080/realms/brain/protocol/openid-connect/certs;
    proxy_set_header Host sso.example.com;
    proxy_set_header X-Forwarded-Host sso.example.com;
    proxy_set_header X-Forwarded-Proto https;
}
```

Adjust the internal upstream and preserve the rest of your existing Keycloak
hostname/proxy/TLS configuration. The alias must serve public JSON directly,
without a login gate or redirect. It changes only the key-serving route, not
Keycloak's issuer. See
[JWKS routing](../AUTHENTICATION.md#jwks-routing-a-required-provider-adaptation).

**One-realm limitation:** `.../realms/brain` and `.../realms/other` resolve to the
same alias on a shared hostname. This exact alias serves only `brain`'s keys.
For multiple independently trusted realms, give each a correctly configured
public issuer hostname and its own alias, or wait for explicit JWKS/discovery
support. Do not assume adding more `trusted_issuers` fixes this collision.
If your deployment has a context prefix such as `/auth`, include that prefix in
both the actual issuer and its computed alias; use discovery as the source of
truth rather than copying these root-context URLs unchanged.

```bash
curl --fail --show-error https://sso.example.com/realms/brain/.well-known/openid-configuration
curl --fail --show-error --include https://sso.example.com/realms/.well-known/jwks.json
```

Confirm discovery's issuer is exactly `https://sso.example.com/realms/brain`,
and the second response is HTTP 200 with the expected signing key in `keys`.
These commands do not follow redirects.

## Configure and connect second-brain-rs

Merge into the complete server configuration:

```toml
[auth]
mode = "jwt"
audience = "second-brain-rs"
trusted_issuers = ["https://sso.example.com/realms/brain"]
discovery_authorization_server = "https://sso.example.com/realms/brain"
jwks_cache_ttl_seconds = 3600
jwt_algorithms = ["RS256"]
development_default_scopes = []
```

Configure the MCP application's registered callback and the following OAuth
settings. No client secret is used for this public PKCE client.

| Setting | Value |
|---|---|
| Client ID | `second-brain-client` |
| Provider metadata | `https://sso.example.com/realms/brain/.well-known/openid-configuration` |
| Authorization | `https://sso.example.com/realms/brain/protocol/openid-connect/auth` |
| Token | `https://sso.example.com/realms/brain/protocol/openid-connect/token` |
| Grant / PKCE | Authorization code / S256 |
| Requested scopes | `openid vault:read` |
| Remote MCP | `https://brain.example.com/mcp` |

Complete login and use the returned **access token** for the
[authenticated check](../AUTHENTICATION.md#obtain-and-check-an-access-token).
Expect `iss` to match the realm URL, `aud` to contain `second-brain-rs`, and `scope`
to contain `vault:read`. Also check nonempty `sub`, future `exp`, and an RS256
header whose `kid` is in the aliased JWKS.

To grant capture or write access, create and attach the additional exact scopes,
then request them explicitly. Assigning a client scope authorizes that client to
request it; plan provider-side user/group restrictions before granting privileged
permissions. Every subject reaches the same server vault. The server's `admin`
scope is not a Keycloak administrator role and does not imply other vault scopes.

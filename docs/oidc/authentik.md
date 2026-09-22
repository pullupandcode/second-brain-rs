# Authentik

Use this with second-brain-rs **v0.5.2** and an Authentik installation matching
the current OAuth2/OIDC provider documentation checked on **2026-09-21**.
These instructions were source/documentation reviewed, not exercised against a
live Authentik deployment. Menu labels can vary by installed release.
Read the [authentication model](../AUTHENTICATION.md) first.

This recipe uses an existing HTTPS Authentik deployment at
`https://auth.example.com`, application slug `brain`, and a public OAuth client
ID of `second-brain-rs`. Your MCP application must support authorization code
with PKCE, a preconfigured client ID, and provider metadata. The provider's login
callback belongs to that application, not the MCP server.

## Register the client and permission

1. In **Applications → Applications**, create an application/provider pair;
   select OAuth2/OpenID Connect and set the application slug to `brain`.
2. Set the client ID to `second-brain-rs`, client type to **Public**, and choose
   your existing authentication/authorization flows. Enable the authorization-code
   grant; disable unneeded grants. Register the MCP client's
   exact redirect URI in strict mode; use its actual callback, without a wildcard.
   For example, `http://127.0.0.1:8765/callback` is suitable only if your client
   really listens there. A public PKCE client has no secret to distribute.
   See [provider creation](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/create-oauth2-provider/).
3. Under **Customization → Property Mappings**, create a **Scope Mapping**:
   name `Second Brain read`, scope name `vault:read`, expression:

   ```python
   return {}
   ```

   An empty dictionary adds no extra claims; the permission is the requested
   scope name. Authentik also uses this pattern in its official
   [custom-scope example](https://docs.goauthentik.io/integrations/services/apple/).
4. Edit the provider's selected scope mappings to include the built-in `openid`
   mapping and your `vault:read` mapping. Explicitly request `openid vault:read`
   from the client. Authentik treats an authorization request that omits `scope`
   as requesting all configured scopes; see [default scopes](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/#default-and-special-scopes).
   Select only permissions this application should grant. Avoid writing a
   user attribute called `scope` only into an ID token. If adding other custom
   claims later, note that the misleadingly named **Include claims in id_token**
   setting controls inclusion of mapped claims in both ID and JWT access tokens
   in current Authentik. See [property mappings](https://docs.goauthentik.io/add-secure-apps/providers/property-mappings/).

In the provider, choose an **RSA signing key** and confirm issued headers use
`RS256`. Leave encryption disabled. Omitting a signing key uses symmetric
signing, which second-brain-rs cannot verify. Keep the default per-provider
issuer mode, yielding `https://auth.example.com/application/o/brain/`.
Authentik exposes its native JWKS at `/application/o/brain/jwks/`.
See [OAuth2 provider signing and endpoints](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/).

Current Authentik creates JWT access tokens, not opaque access tokens. Its
standard access-token construction uses the OAuth client ID as `aud` and joins
granted scopes into the `scope` string. This recipe deliberately chooses client
ID `second-brain-rs`, so that the server's audience can match without a custom
claim override. See the provider's
[token implementation](https://github.com/goauthentik/authentik/blob/main/authentik/providers/oauth2/id_token.py).
Do not replace the ID token's audience with an unrelated API audience: that can
break the OIDC client's own validation. If using several OAuth clients, plan
their resource audience mapping explicitly rather than assuming each client ID
is interchangeable.

## Add the JWKS alias

In the existing HTTPS Nginx `server` for **auth.example.com**, add this exact
location. Adjust `127.0.0.1:9000` to the internal Authentik upstream. Retain your
normal Authentik proxy locations and TLS configuration.

```nginx
location = /application/o/brain/.well-known/jwks.json {
    proxy_pass http://127.0.0.1:9000/application/o/brain/jwks/;
    proxy_set_header Host auth.example.com;
    proxy_set_header X-Forwarded-Host auth.example.com;
    proxy_set_header X-Forwarded-Proto https;
}
```

This serves the public key set directly at the URL computed by v0.5.2. It does
not change the provider issuer or sign tokens. Exempt this public-key location
from any additional proxy login gate. Do not redirect it to the native URL.
See [why the alias is necessary](../AUTHENTICATION.md#jwks-routing-a-required-provider-adaptation).

```bash
curl --fail --show-error --include https://auth.example.com/application/o/brain/.well-known/jwks.json
```

Expect HTTP 200 and JSON containing `keys`; check that the JWT's `kid` occurs
in a compatible RSA signing key. This command intentionally does not follow redirects.

## Configure second-brain-rs and the MCP client

Merge into your complete server configuration:

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

Configure the OAuth-capable MCP client with:

| Setting | Value |
|---|---|
| Client ID | `second-brain-rs` |
| Provider metadata | `https://auth.example.com/application/o/brain/.well-known/openid-configuration` |
| Authorization endpoint | `https://auth.example.com/application/o/authorize/` |
| Token endpoint | `https://auth.example.com/application/o/token/` |
| Grant / PKCE | Authorization code / S256 |
| Requested scopes | `openid vault:read` |
| Remote MCP URL | `https://brain.example.com/mcp` |

Use the returned **access token** in the [authenticated check](../AUTHENTICATION.md#obtain-and-check-an-access-token).
Its `iss` must include the final slash, `aud` must include `second-brain-rs`, and
`scope` must include `vault:read`; nonempty `sub`, future `exp`, and a verifiable
signature are also required.

## Control who may request more scopes

Repeat the scope-mapping step for needed permissions such as `vault:capture` or
`daily:append`, select them on the provider, and request them from the client.
Do not attach all eight permissions as a convenience. By default, users allowed
into the application can request its configured scopes. Bind group/user policies
to the application; for a privileged optional scope, an expression policy can
check `request.context["oauth_scopes"]` and reject unauthorized membership.
Authentik documents this in
[scope authorization](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/).

If your client requires refresh tokens, enable/request `offline_access` according
to your installed provider's policy. The refresh token goes only to Authentik's
token endpoint; second-brain-rs never accepts it. Recheck the access-token claims
after any scope, audience, signing-key, or issuer-mode change.

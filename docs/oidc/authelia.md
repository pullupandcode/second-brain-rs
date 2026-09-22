# Authelia

This guide targets second-brain-rs **v0.5.2** and the current Authelia configuration
schema documented on **2026-09-21**, including signed access tokens, custom scopes,
and `requested_audience_mode`. It assumes an existing, functioning Authelia
installation with users, MFA, storage, sessions, and HTTPS. These are configuration
instructions derived from official docs/source, **not a live integration test**.
An older Authelia release may lack these options; check its matching docs.

## Compatibility: a root-hosted issuer does not work directly

Authelia commonly signs `iss = "https://auth.example.com"` without a trailing
slash. second-brain-rs v0.5.2 stores configured issuers as normalized Rust URLs,
turning this into `https://auth.example.com/`, then requires an exact string
match. **The two values do not match**, even if you omit the slash in TOML.
Authelia constructs its issuer from the forwarded scheme, host, and routing
base path; the root path is empty. See its
[issuer construction](https://github.com/authelia/authelia/blob/master/internal/middlewares/authelia_context.go)
and [signed-session claims](https://github.com/authelia/authelia/blob/master/internal/oidc/session.go).

A JWKS alias alone cannot repair this. Do not rewrite token payloads, change the
issuer arbitrarily in TOML, or disable signature checks. For an existing root
installation, direct use needs a future server compatibility fix, or a deliberate
provider topology change. This documentation does not make that code change.

The recipe below uses Authelia's **supported subpath deployment** at `/authelia`.
A non-root issuer `https://auth.example.com/authelia` is preserved by Rust. Treat
this as a deployment choice, not an incidental proxy rewrite: moving an existing
provider changes URLs used by other clients. Verify the discovery issuer and an
issued access token before treating the setup as compatible.

## 1. Establish the subpath and matching public URL

Merge these values into the existing configuration; retain your session secrets,
cookie options, users, storage, notification, and access policies:

```yaml
server:
  address: 'tcp://127.0.0.1:9091/authelia'
session:
  cookies:
    - domain: 'example.com'
      authelia_url: 'https://auth.example.com/authelia'
```

The address path enables the subpath route. The cookie entry must match the
public Authelia URL; do not replace unrelated cookie entries. The local address
assumes Nginx runs on the same machine; use the appropriate private container
address for a container deployment. See
[server paths](https://www.authelia.com/configuration/miscellaneous/server/) and
[session cookie URLs](https://www.authelia.com/configuration/session/introduction/).

In the existing TLS Nginx virtual host for **auth.example.com**, preserve the
prefix to the upstream and add the public-key alias:

```nginx
location = /authelia {
    return 302 /authelia/$is_args$args;
}

location /authelia/ {
    proxy_pass http://127.0.0.1:9091;
    proxy_set_header Host auth.example.com;
    proxy_set_header X-Forwarded-Host auth.example.com;
    proxy_set_header X-Forwarded-Proto https;
}

location = /.well-known/jwks.json {
    proxy_pass http://127.0.0.1:9091/authelia/jwks.json;
    proxy_set_header Host auth.example.com;
    proxy_set_header X-Forwarded-Host auth.example.com;
    proxy_set_header X-Forwarded-Proto https;
}
```

Retain your normal Authelia proxy hardening, including client-IP forwarding as
appropriate to your trusted proxy chain. Do not add a proxy login requirement to
the JWKS alias. The alias must return JSON directly, without a redirect. The
first `proxy_pass` has **no URI suffix**, so it preserves `/authelia/`.
See [JWKS routing](../AUTHENTICATION.md#jwks-routing-a-required-provider-adaptation).

Use the subpath for discovery, authorization, and token requests consistently.
Authelia also serves root routes when a subpath is configured; mixing root and
subpath requests can produce the wrong issuer for this recipe.

## 2. Configure a JWT access-token client

Under the existing `identity_providers.oidc`, retain a strong `hmac_secret` and
configure an RSA signing key. If you need a new key, generate it privately:

```bash
umask 077
authelia crypto pair rsa generate --bits 4096 --directory ./oidc-keys
```

Use the generated `private.pem` only in Authelia's protected configuration/secret
storage. Never copy a private key to second-brain-rs or expose it through JWKS.
See the [key-generation CLI](https://www.authelia.com/reference/cli/authelia/authelia_crypto_pair_rsa_generate/).

The following is a **merge fragment**, not a complete Authelia configuration.
Replace the PEM placeholder and callback with real values. Preserve existing
keys and clients. The scope definition grants a scope name without adding
userinfo attributes; `scope` is already part of the JWT access-token profile.
See [provider JWKS and scope configuration](https://www.authelia.com/configuration/identity-providers/openid-connect/provider/).

```yaml
identity_providers:
  oidc:
    # Keep your existing hmac_secret or configured secret injection.
    jwks:
      - algorithm: 'RS256'
        use: 'sig'
        key: |
          -----BEGIN PRIVATE KEY-----
          REPLACE_WITH_THE_GENERATED_PRIVATE_KEY_BODY
          -----END PRIVATE KEY-----
    scopes:
      'vault:read':
        claims: []
    clients:
      - client_id: 'second-brain-client'
        client_name: 'Second Brain MCP client'
        public: true
        authorization_policy: 'two_factor'
        redirect_uris:
          - 'http://127.0.0.1:8765/callback'
        scopes:
          - 'openid'
          - 'vault:read'
        audience:
          - 'https://brain.example.com'
        requested_audience_mode: 'implicit'
        grant_types:
          - 'authorization_code'
        response_types:
          - 'code'
        require_pkce: true
        pkce_challenge_method: 'S256'
        token_endpoint_auth_method: 'none'
        access_token_signed_response_alg: 'RS256'
```

Authelia defaults to opaque access tokens; `access_token_signed_response_alg:
'RS256'` enables JWT access tokens. Leave access-token encryption disabled.
`implicit` audience mode supplies the allowed audience when the client omits
`resource`/`audience`; if supplied, the requested resource must be
`https://brain.example.com`. A public client uses PKCE and no client secret.
See [client options](https://www.authelia.com/configuration/identity-providers/openid-connect/clients/).

Only users allowed by this client's authorization policy should receive these
permissions. Add further scope definitions and client scope entries only when
needed; `admin` is a separate privileged permission. Configure provider policy
before allowing more powerful clients. second-brain-rs does not interpret
Authelia groups or userinfo attributes as permissions.

## 3. Verify discovery, then configure the server

```bash
curl --fail --show-error https://auth.example.com/authelia/.well-known/openid-configuration
curl --fail --show-error --include https://auth.example.com/.well-known/jwks.json
```

Before proceeding, the first response's issuer must be exactly
`https://auth.example.com/authelia`, and the second must be HTTP 200 with a JSON
`keys` array. Stop and investigate if the issuer has a different path or slash;
do not assume this untested topology matches your installed release.
Authelia's native JWKS, authorization, and token endpoints are documented in its
[OIDC integration guide](https://www.authelia.com/integration/openid-connect/introduction/);
the subpath prefixes those routes in this deployment.

Merge into second-brain-rs configuration:

```toml
[auth]
mode = "jwt"
audience = "https://brain.example.com"
trusted_issuers = ["https://auth.example.com/authelia"]
discovery_authorization_server = "https://auth.example.com/authelia"
jwks_cache_ttl_seconds = 3600
jwt_algorithms = ["RS256"]
development_default_scopes = []
```

The missing trailing slash is intentional: the actual signed issuer stays
unchanged, while URL joining computes the root `/.well-known/jwks.json` alias.
The audience here is the public resource URL, matching the allowed audience
above; it differs from the arbitrary `second-brain-rs` example in other guides.

Configure the MCP client's client ID as `second-brain-client`, callback as
registered, scopes as `openid vault:read`, and PKCE S256:

| Setting | URL |
|---|---|
| Provider metadata | `https://auth.example.com/authelia/.well-known/openid-configuration` |
| Authorization | `https://auth.example.com/authelia/api/oidc/authorization` |
| Token | `https://auth.example.com/authelia/api/oidc/token` |
| Remote MCP | `https://brain.example.com/mcp` |

Complete the login/code exchange with your OAuth-capable client and run the
[access-token check](../AUTHENTICATION.md#obtain-and-check-an-access-token).
Confirm `iss`, resource `aud`, `scope` containing `vault:read`, nonempty `sub`,
future `exp`, and RS256 signing. An ID token or opaque token cannot substitute
for the JWT access token required by this setup.

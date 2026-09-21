# Production deployment and operations

[User guide](USER_GUIDE.md) · [Configuration](CONFIGURATION.md) · [Authentication](AUTHENTICATION.md)

## Deployment layout

Run one unprivileged server process for each vault/state pair. Put HTTPS in front
of a loopback listener and use JWT authentication. A typical layout is:

```text
MCP client → HTTPS brain.example.com → reverse proxy → 127.0.0.1:3000
                  ↓ OAuth login/token issuance
             identity provider

/opt/second-brain/second-brain-rs   executable
/etc/second-brain/config.toml      configuration
/srv/second-brain/vault/           existing Markdown vault
/var/lib/second-brain/             index, audit database and audit archives
```

Create the vault directory first. Give the service account read/write access to
its vault, state, and any separately configured SQLite/archive locations. Keep
state outside synchronized vault contents. Protect configuration and backups from
other local users. Multiple writer processes are not coordinated; do not run
replicas against the same vault or state files.

Start with [config.example.toml](../config.example.toml), use absolute filesystem
paths, and configure `listen = "127.0.0.1:3000"` and
`public_base_url = "https://brain.example.com"`. Replace the example authentication
values using the [Authentik](oidc/authentik.md), [Authelia](oidc/authelia.md), or
[Keycloak](oidc/keycloak.md) guide. The provider JWKS compatibility configuration
is necessary in addition to this server's reverse proxy.

## HTTPS reverse proxy

For an installed Caddy server, add the following site to its Caddyfile. Point DNS
at the proxy and allow the ports needed by your chosen certificate issuance
method. Caddy's [HTTPS quick start](https://caddyserver.com/docs/quick-starts/https)
explains certificate provisioning.

```caddyfile
brain.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

For this HTTP upstream, Caddy preserves incoming headers including Host and
Authorization by default; see its [reverse proxy documentation](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#headers).
Other proxies must forward the original public Host and Bearer header unchanged.
The server accepts its configured public Host. `public_base_url` does not mount a
path prefix: publish `/mcp`, `/.well-known/oauth-protected-resource`, and, as
appropriate for your network, `/tools` and `/healthz` at the origin root.

Do not replace API requests with a proxy login HTML page or inject development
scope tokens. OAuth belongs at the identity provider; the server validates the
client's access token. Its HTTP surface uses stateless JSON MCP responses.
Forwarded client addresses are not trusted as authorization identities; logs
normally report the directly connected proxy as the peer.

Allow server outbound HTTPS to the derived JWKS endpoints. They must return JSON
directly, without redirects. Use trusted certificates. The server does not read
the provider discovery document's `jwks_uri`; follow the provider guide's exact
path mapping rather than inventing a `jwks_url` configuration key.

## Linux service example

After installing the executable and creating a `second-brain` user/group with
access to the paths above, save this as
`/etc/systemd/system/second-brain.service`:

```ini
[Unit]
Description=Second Brain MCP server
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=second-brain
Group=second-brain
WorkingDirectory=/var/lib/second-brain
ExecStart=/opt/second-brain/second-brain-rs --config /etc/second-brain/config.toml
Environment=RUST_LOG=info
Restart=on-failure
RestartSec=5
KillSignal=SIGINT
TimeoutStopSec=30
NoNewPrivileges=true
UMask=0077

[Install]
WantedBy=multi-user.target
```

The binary listens for Ctrl+C/SIGINT for graceful shutdown, hence `KillSignal`.
Systemd can terminate it after the stop timeout. Adapt paths and permissions to
your installation; this example does not create its service account or directories.
See systemd's [service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html)
and [shutdown signal](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html)
references for service-manager behavior.

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now second-brain
sudo systemctl status second-brain
sudo journalctl -u second-brain -f
```

On macOS or Windows, first validate the foreground invocation from
[getting started](GETTING_STARTED.md), then configure your chosen service manager
with the same executable, explicit `--config` path, working directory and account.
The binary does not install itself as an operating-system service. No official
container image is supplied by the executable release workflow.

## Acceptance checks

1. Check the process is running and `GET /healthz` returns `{"ok":true}`.
2. Fetch `/.well-known/oauth-protected-resource` through public HTTPS; confirm its
   resource and authorization-server values match your deployment.
3. Obtain a real access token with a matching issuer/audience and `vault:read`.
   Use it on `GET /tools` and `read_note` for a disposable known note. Health and
   MCP initialization can succeed before authentication and do not prove JWT setup.
4. In a test vault, exercise a write with its own scope, reread the note and check
   `list_write_recovery_diagnostics` with `admin`. Do not use production notes to
   test delete operations.
5. Restart once to confirm paths, permissions, index rebuild and provider access
   work under the service account, not just your login shell.

See [development request helpers](DEVELOPMENT.md#reusable-bash-request-helpers)
for raw HTTP requests; use a real JWT and public URL in production. Protocol
errors may be inside HTTP 200 responses, so inspect the JSON-RPC envelope.

## Backups and restoration

Back up the **vault and complete state directory together**, including framework
metadata, skill notes/maps, SQLite databases and audit archives. Include any
configured database/archive path outside those directories. The search index is
rebuildable; audit and provenance history are not a disposable cache. Vault
synchronization alone is not a backup of server state.

For a simple consistent backup, stop the server and pause other editors/sync
writers, copy or snapshot the vault and state, then resume them. Do not copy only
a live SQLite main database while ignoring its journal/WAL files. Keep a copy of
the configuration and the exact release version with the backup.

Test restoration into separate directories before replacing a live instance.
Stop the old instance, restore the matching vault/state snapshot and permissions,
point a configuration at those absolute paths, and start the matching binary.
Check a read, search, framework composition and audit diagnostics. Never start
both restored and original writers on the same directories.

## Updates and routine maintenance

Download an immutable release and verify its archive checksum as described in
[getting started](GETTING_STARTED.md). Read [CHANGELOG](../CHANGELOG.md), back up,
stop the process, replace the executable, then start and repeat acceptance checks.
Preserve your configuration and state; do not overwrite them with sample files.
For rollback, retain the previous executable and its matching backup. Do not
assume an older binary can read a future version's state format.

Search/link indexes rebuild at startup and after server mutations. External edits
are visible to direct reads, but search may remain stale until a rebuild; there
is no background watcher. Conflict quarantine is a startup snapshot: resolve sync
conflicts externally and restart before retrying affected writes.

Audit rotation runs at startup when configured and can defer while attempts are
incomplete. There is no automatic archive purge or recovery replay. Inspect
recovery diagnostics and the filesystem before deciding whether to retry an
interrupted write. Soft deletion can leave both source and trash names if
interrupted between publication and unlink. Keep `logging.log_args = false` unless
you deliberately want note content in logs. See [troubleshooting](TROUBLESHOOTING.md)
and [configuration](CONFIGURATION.md) for these operational limits.

# Local development mode

[User guide](USER_GUIDE.md) · [Configuration](CONFIGURATION.md) · [Common use cases](USE_CASES.md)

Development mode removes the need for an identity provider. It does **not**
authenticate users: any caller can ask for any supported scope in a header.
Use loopback, a disposable vault, and a trusted local machine. An empty fallback
scope list is useful for debugging but does not make this mode secure.

## Complete local configuration

Create `demo-vault/` before starting. Save this as `config.development.toml` in your
working directory, or copy [the example file](../examples/config.development.toml).
The example's relative vault/state paths are resolved from the process working
directory. All required production-shaped configuration fields remain necessary;
the placeholder issuer is never contacted in development mode.

```toml
# Local demonstration only. Run from the directory containing demo-vault/.
# This mode does not authenticate users; callers can choose their own scopes.
listen = "127.0.0.1:3000"
public_base_url = "http://127.0.0.1:3000"
vault_path = "./demo-vault"
state_path = "./demo-state"

[auth]
mode = "development"
audience = "second-brain-rs"
# Required configuration fields; no issuer requests are made in development mode.
trusted_issuers = ["https://example.invalid/"]
discovery_authorization_server = "https://example.invalid/"
jwks_cache_ttl_seconds = 3600
jwt_algorithms = ["RS256"]
development_default_scopes = []

[index]
watcher_polling = false
ignored_globs = [".obsidian/workspace*", ".trash/**", "**/.DS_Store", "**/*.sync-conflict-*"]
blocked_paths = []

[security]
blocked_paths = ["Private/**"]

[writes]
cooldown_seconds = 0

[deletes]
trash_path = ".trash/mcp"

[audit]
retention_max_rows = 0

[framework]
schema_path = "_meta/framework.yaml"

[skills]
map_paths = []

[daily_note]
capture_default_pattern = "B"

[ocr]
enabled = false

[logging]
log_args = false
```

Start the downloaded executable from that directory:

```bash
./second-brain-rs --config config.development.toml
```

From a source checkout use:

```bash
cargo +stable run --locked -- --config config.development.toml
```

On Windows, use `.\second-brain-rs.exe --config config.development.toml`.
Create a readable test note such as `demo-vault/Welcome.md`, then use another
terminal for the requests below. The zero write cooldown is intentional for this
demo; the production example uses two seconds.

## First request and explicit scopes

```bash
curl -fsS http://127.0.0.1:3000/healthz
curl -fsS -H 'Authorization: Bearer scope=vault:read' http://127.0.0.1:3000/tools
```

The second request lists tools visible to `vault:read`. Multiple scope names are
separated by spaces, for example `Bearer scope=vault:read vault:write`. Matching is
case-sensitive: use exactly `Bearer ` and `scope=`. `admin` grants only its own
tools, not read/write/capture/prompt permissions.

PowerShell equivalent:

```powershell
$SbHeaders = @{ Authorization = 'Bearer scope=vault:read'; Accept = 'application/json, text/event-stream' }
Invoke-RestMethod http://127.0.0.1:3000/tools -Headers $SbHeaders
$SbBody = @{
  jsonrpc = '2.0'; id = 1; method = 'tools/call'
  params = @{ name = 'read_note'; arguments = @{ path = 'Welcome.md' } }
} | ConvertTo-Json -Depth 10
Invoke-RestMethod http://127.0.0.1:3000/mcp -Method Post -Headers $SbHeaders `
  -ContentType 'application/json' -Body $SbBody `
  -UserAgent 'second-brain-docs'
```

The `Accept` header covers both media types expected by Streamable HTTP clients.

## Reusable Bash request helpers

These examples require `curl` and `jq`. Keep this shell open while following the
[use cases](USE_CASES.md). `sb_rpc` prints the raw JSON-RPC response; `sb_call`
wraps a tool name and JSON arguments. HTTP 200 can still contain a JSON-RPC error;
these helpers print the envelope unchanged. Inspect `.error` before consuming `.result`.

```bash
export SB_MCP_URL='http://127.0.0.1:3000/mcp'
export SB_TOKEN='scope=vault:read'

sb_rpc() {
  jq -nc --arg method "$1" --argjson params "$2" \
    '{jsonrpc:"2.0",id:1,method:$method,params:$params}' |
    curl -fsS "$SB_MCP_URL" \
      -H "Authorization: Bearer $SB_TOKEN" \
      -H 'Content-Type: application/json' \
      -H 'Accept: application/json, text/event-stream' \
      --data-binary @-
}

sb_call() {
  sb_rpc tools/call "$(jq -nc --arg name "$1" --argjson arguments "$2" \
    '{name:$name,arguments:$arguments}')"
}

sb_rpc initialize '{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"docs","version":"1"}}' | jq .
sb_rpc tools/list '{}' | jq .
sb_call read_note '{"path":"Welcome.md"}' | jq .
```

This server's HTTP transport is stateless. A real MCP client performs its own
initialization; identity comes from the current request header, not an earlier
initialization or session. Sending an initialized notification is supported but
does not establish authorization.

Typical object results are in `.result.structuredContent`; a `read_note` hash is
`.result.structuredContent.currentSha256`. Array results use
`.result.structuredContent.result`. The response also includes text content for
clients without structured-result support. A safe extraction pattern is:

```bash
SB_RESPONSE=$(sb_call read_note '{"path":"Welcome.md"}')
printf '%s\n' "$SB_RESPONSE" | jq -e 'if .error then error(.error.message) else .result.structuredContent end'
```

To perform the non-destructive writing examples in a local disposable vault:

```bash
export SB_TOKEN='scope=vault:read vault:write vault:capture daily:append skills:read admin'
```

Deletion examples require adding `vault:delete` or `vault:delete:hard` explicitly.
Do not paste a development header into a production client. For JWT mode replace
`SB_TOKEN` with an actual provider-issued access token and set the HTTPS MCP URL;
the same request helper then applies.

## Reusable PowerShell request helper

```powershell
$SbMcpUrl = 'http://127.0.0.1:3000/mcp'
$SbToken = 'scope=vault:read'
function Invoke-SbTool([string]$Name, [hashtable]$Arguments) {
  $Headers = @{
    Authorization = "Bearer $SbToken"
    Accept = 'application/json, text/event-stream'
  }
  $Body = @{
    jsonrpc = '2.0'; id = 1; method = 'tools/call'
    params = @{ name = $Name; arguments = $Arguments }
  } | ConvertTo-Json -Depth 20
  $Response = Invoke-RestMethod $SbMcpUrl -Method Post -Headers $Headers `
    -ContentType 'application/json' -Body $Body
  if ($Response.error) { throw ($Response.error | ConvertTo-Json -Depth 10) }
  return $Response.result.structuredContent
}
Invoke-SbTool 'read_note' @{ path = 'Welcome.md' }
```

This PowerShell helper returns structured content rather than the raw envelope.
Translate a Bash `sb_call` JSON argument object into a PowerShell hashtable; the
tool names and argument keys stay the same.

## Default scopes and surprising results

`development_default_scopes` is a fallback, not a maximum. Missing headers,
malformed headers, `Bearer other`, and explicit claims containing **no known
scope** all use the fallback. A claim with at least one known scope uses that
parsed set instead; it does not add the fallback. Unknown scopes are discarded.

With the example's empty fallback, a request without a header can return an empty
successful `/tools` listing. This is expected and is not JWT authentication.
Adding `development_default_scopes = ["vault:read"]` makes headerless local clients
convenient but grants those read capabilities to every caller reaching the port.
Restart after changing the file.

Development mode rejects non-loopback listen addresses unless
`SECOND_BRAIN_ALLOW_DEV_AUTH=1` is set. Do not set that variable for internet-facing
or shared-network deployments. Use JWT mode for remote access. To reach a remote
development process, prefer an authenticated SSH tunnel to a loopback listener
on a host you control rather than exposing its port.

## Developer iteration

`RUST_LOG=debug ./second-brain-rs --config config.development.toml` increases JSON
log verbosity. Keep `[logging] log_args = false` unless you deliberately want note
arguments in logs. On PowerShell set `$env:RUST_LOG = 'debug'` before starting.
There is no hot reload of TOML and no automatic filesystem watcher. Restart for
configuration or external-index changes; use `skills_reload` and
`framework_reload` only for their respective vault-backed definitions.

For implementation tests, toolchains and contribution rules see
[CONTRIBUTING.md](../CONTRIBUTING.md). Development **authentication** and developing
the Rust implementation are separate choices; a downloaded binary supports the
same local mode.

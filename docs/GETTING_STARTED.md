# Getting started

[User guide](USER_GUIDE.md) · [Development mode](DEVELOPMENT.md) · [Production deployment](DEPLOYMENT.md)

## 1. Choose a build

The [v0.5.2 release](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.5.2)
contains the following native archives and `SHA256SUMS`. New versions use the same
naming pattern. You do not need Rust to use an executable archive.

| Computer | Asset suffix after `second-brain-rs-v0.5.2-` | Tested build platform |
|---|---|---|
| Linux x86-64 | `x86_64-unknown-linux-gnu.tar.gz` | Ubuntu 24.04; GNU/glibc runtime |
| Mac with Apple Silicon | `aarch64-apple-darwin.tar.gz` | macOS 15 |
| Intel Mac | `x86_64-apple-darwin.tar.gz` | macOS 15 Intel |
| Windows x86-64 | `x86_64-pc-windows-msvc.zip` | Windows Server 2022 |

Older Linux distributions may lack the required glibc symbols. Alpine/musl and
Linux ARM do not have a supplied binary; build from source on a supported Rust
environment. The Mac executables are not Developer-ID signed/notarized. Follow
your organization's policy for downloaded software; do not disable platform
security checks globally.

Download only the archive for your computer and `SHA256SUMS`. Compare that file's
SHA-256 with its named entry before extracting. These are checksums of the
**archive**, not of the extracted executable.

Linux example (Bash):

```bash
SB_VERSION=0.5.2
SB_TARGET=x86_64-unknown-linux-gnu
SB_ARCHIVE="second-brain-rs-v${SB_VERSION}-${SB_TARGET}.tar.gz"
SB_RELEASE="https://github.com/pullupandcode/second-brain-rs/releases/download/v${SB_VERSION}"
curl -fLO "$SB_RELEASE/$SB_ARCHIVE" &&
  curl -fLO "$SB_RELEASE/SHA256SUMS" &&
  awk -v name="$SB_ARCHIVE" '$2 == name { print }' SHA256SUMS > selected.sha256 &&
  test -s selected.sha256 &&
  sha256sum -c selected.sha256 &&
  tar -xzf "$SB_ARCHIVE" &&
  cd "second-brain-rs-v${SB_VERSION}-${SB_TARGET}"
```

For a Mac use `SB_TARGET=aarch64-apple-darwin` or `x86_64-apple-darwin`, and replace
`sha256sum -c selected.sha256` with `shasum -a 256 -c selected.sha256`.
Stop if the checksum does not match; do not extract and run a mismatched file.

Windows example (PowerShell, in a new download directory):

```powershell
$SbVersion = '0.5.2'
$SbName = "second-brain-rs-v$SbVersion-x86_64-pc-windows-msvc"
$SbRelease = "https://github.com/pullupandcode/second-brain-rs/releases/download/v$SbVersion"
Invoke-WebRequest "$SbRelease/$SbName.zip" -OutFile "$SbName.zip"
Invoke-WebRequest "$SbRelease/SHA256SUMS" -OutFile SHA256SUMS
$SbLine = Get-Content SHA256SUMS | Where-Object { ($_ -split '\s+')[1] -eq "$SbName.zip" }
if (@($SbLine).Count -ne 1) { throw 'Missing or ambiguous checksum' }
$SbExpected = ($SbLine -split '\s+')[0]
if ((Get-FileHash "$SbName.zip" -Algorithm SHA256).Hash -ne $SbExpected) { throw 'Checksum mismatch' }
Expand-Archive "$SbName.zip" -DestinationPath .
Set-Location $SbName
```

An archive contains the executable, `config.example.toml`, README and project
licenses. These longer guides and the development config are in the source
repository; they are not all included in the binary archive.

### Alternative: build from source

Install a current stable Rust toolchain and the native compiler/linker required
by your platform. The project uses Rust 2024 and includes native cryptographic
code; CMake and a C/C++ toolchain may be required when building dependencies.
On Windows use the MSVC toolchain and Visual Studio C++ build tools; on macOS
install Xcode Command Line Tools. A packaged binary avoids these build prerequisites.

```bash
git clone https://github.com/pullupandcode/second-brain-rs.git
cd second-brain-rs
git checkout v0.5.2
cargo +stable build --release --locked
```

Run `./target/release/second-brain-rs` in subsequent Unix examples, or
`.\target\release\second-brain-rs.exe` on Windows. The CLI takes `--config FILE`;
there is no implemented `--help` or `--version` command. MCP initialization reports
the running version.

## 2. Prepare a vault and state directory

Start with a small test vault. Create its directory before starting the process.
Keep the state directory outside your vault and outside its synchronization rules.
The process needs permission to read the vault and write state. Writing tools
also need permission to modify the relevant vault files and directories.

For the local example, run from a working directory that will own these paths:

```bash
mkdir -p demo-vault demo-state
printf '# Welcome\n\nA note for the first connection.\n' > demo-vault/Welcome.md
```

PowerShell equivalent:

```powershell
New-Item -ItemType Directory -Force demo-vault, demo-state | Out-Null
Set-Content -Path demo-vault/Welcome.md -Value "# Welcome`n`nA note for the first connection."
```

For a real vault, use absolute paths such as `/srv/second-brain/vault` and
`/var/lib/second-brain`, or TOML literal strings such as `'C:\Notes\Vault'` and
`'C:\SecondBrain\State'`. Filesystem-relative paths resolve from the process's
working directory, not from the configuration file's directory.

## 3. Select the authentication mode

- **Local evaluation:** follow [Development mode](DEVELOPMENT.md). It includes a
  complete TOML file and executable HTTP examples. Keep it on loopback and use a
  disposable vault; callers can choose their own scopes.
- **Production:** copy `config.example.toml` to `config.local.toml`, set absolute
  vault/state paths and an HTTPS `public_base_url`, then follow
  [Authentication](AUTHENTICATION.md) and the provider-specific page. The example
  issuer URLs are placeholders, not a working identity service.

All required tables must be present even in development mode. Copying only
`[auth]` into an otherwise empty file is not sufficient.

## 4. Start and check the service

The commands below use the production file name. For the local demo, substitute
`config.development.toml` as shown in the development guide.

```bash
./second-brain-rs --config config.local.toml
```

PowerShell:

```powershell
.\second-brain-rs.exe --config config.local.toml
```

Keep that terminal open. In another terminal:

```bash
curl -fsS http://127.0.0.1:3000/healthz
curl -fsS http://127.0.0.1:3000/.well-known/oauth-protected-resource
```

Health returns `{"ok":true}`. Discovery advertises the resource URL, authorization
server and eight supported scopes. Both endpoints are public; their success does
not prove that a JWT is valid. Test an authenticated `/tools` request and an actual
`read_note` call using [Development](DEVELOPMENT.md) or [Authentication](AUTHENTICATION.md).
Press Ctrl+C to stop an interactive process. Configuration edits require a restart.

## 5. Connect an MCP client

Set the client's transport to **Streamable HTTP**, URL to
`http://127.0.0.1:3000/mcp` locally or `https://brain.example.com/mcp` in production,
and send an `Authorization: Bearer ...` header on every request. Raw HTTP calls
should advertise `Accept: application/json, text/event-stream` and send JSON.

| Client field | Value |
|---|---|
| Transport | Streamable HTTP; not stdio or a legacy SSE URL |
| Server URL | Public base URL plus `/mcp` |
| Authentication | A JWT access token, or a local-only development header |
| Scopes | Explicit workflow scopes; `admin` does not include the others |
| Refresh/login | Managed by your OAuth-capable client or token acquisition tooling |

Client configuration formats vary, so translate these fields into that client's
supported settings rather than copying an invented universal JSON format. A client
that cannot send a Bearer header or use your provider's login flow needs additional
client-side integration. Discovery alone does not provide dynamic registration.
Never put a confidential OAuth client secret into a distributed desktop client.

Continue with [common workflows](USE_CASES.md) and the [tool reference](TOOLS.md).

# Contributing to second-brain-rs

Thanks for your interest in contributing!

## Coding standards

All Rust code in this repository must follow
[RUST_GUIDELINES.md](RUST_GUIDELINES.md). Before opening a PR, review the
Quick Reference Checklist in that document; reviewers enforce it.
Use the current stable toolchain for compilation, tests, and Clippy.

## Development prerequisites

- Current stable Rust toolchain — `edition = "2024"`.
- `cargo-deny` (for the `ci deny` step): `cargo install cargo-deny`.
- `cargo-audit` (for the `ci audit` step): `cargo install cargo-audit`.
- A nightly toolchain is only required for `cargo fmt` (the `rustfmt.toml`
  uses a couple of unstable options).

## Verification steps

Run locally before opening a PR:

```bash
cargo +nightly fmt --all -- --check
cargo +stable clippy --all-targets --all-features -- -D warnings
cargo +stable test --all-features
cargo +stable deny check
cargo +stable audit
```

All five must pass. Authentication/configuration/input-validation changes also
require focused mutation testing with a passing unmutated baseline and documented
survivors. Use `RUSTUP_TOOLCHAIN=stable cargo mutants ...` so its child builds use
the same toolchain as CI. Public API changes require `cargo semver-checks` against
the preceding release; classify intentional pre-1.0 minor breaks in the changelog.

Test tiers: module tests and property tests are local/unit checks; integration
suites use temporary vaults, local mock identity providers and loopback HTTP.
They do not require a live account. Run socket tests in an environment permitting
local listeners; a sandbox-denied bind is a blocked check, not a passing result.

## Pull request checklist

- [ ] Commit follows the [Conventional Commits](#commit-convention) format.
- [ ] `fmt`, `clippy`, and `test` all clean.
- [ ] New public items are documented (rustdoc, `#[must_use]` where
      appropriate).
- [ ] User-visible changes recorded in CHANGELOG; a release item updates the
      crate and lockfile version together under its versioned entry.
- [ ] Behavior-changing tests demonstrate the failure before the fix.
- [ ] Both independent reviewers approve the exact integrated head; GitHub's
      separate account-review requirements and CI gates are satisfied.
- [ ] No `unwrap()` / `expect()` / `panic!` in library code paths.
- [ ] No internal error details leaked in HTTP responses.

## Commit convention

```
<type>(<scope>): <subject>

<body>
```

**Types**: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `ci`.
**Scopes** (one of the top-level modules): `transport`, `auth`, `config`, `observability`,
`vault`, `framework`, `runtime`, `mcp`, `skills`, `ocr`, `docs`, `release`.

Examples:

- `feat(auth): verify JWT signatures with cached issuer keys`
- `fix(vault): reject symlink escapes before writing`
- `docs(framework): describe overlay registration`

## Coding rules (non-negotiable)

- `unsafe_code` is forbidden at the crate level.
- No `unwrap()` / `expect()` / `panic!` / `todo!` in library code.
- Accept `&str` not `&String`; `&[T]` not `&Vec<T>`.
- No `.clone()` to satisfy the borrow checker.
- No blocking I/O inside `async fn`.
- All HTTP responses must carry OWASP security headers set by the
  middleware stack.
- Secrets go through `secrecy::SecretString` / `secrecy::SecretBox`.

## Adding a cargo feature

1. Gate the new optional dependency with `optional = true`.
2. Add a `[features]` entry that activates it via `dep:<crate>`.
3. Document the feature in `README.md` and `docs/USER_GUIDE.md`.
4. Configure docs.rs to exercise the feature when it introduces new public items.
5. Extend CI: `cargo +stable test --features <new-feature>` matrix entry.

## Licensing

Contributions are dual-licensed under MIT OR Apache-2.0, matching the
crate. By opening a PR you agree to this licensing.
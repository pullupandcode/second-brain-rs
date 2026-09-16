# Scope D: production authentication and delivery documentation

Reference: second-brain-mcp v1.1.1 at
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`; adopted issues #37–#39, #41–#42.

## Test-driven evidence

Before production changes, `cargo +stable test --test auth_jwt` failed both
initial tests: `wrong_signature` returned a development AuthContext, and the
JWT-mode factory accepted `Bearer scope=vault:read admin`. The fixture tokens
were signed with the pinned reference's jose implementation. Fixture private
keys are test-only and have no deployment use.

After implementation, the first seven focused tests passed. A subsequent signed
`exp == now` boundary test failed: jsonwebtoken's default strict-less comparison
accepted the token at its expiration second. An initial exclusive expiry margin
fixed integer expiry, but independent review later found that library rounding
still diverged for fractional NumericDates. Signed regression tests then failed
for issuer arrays, malformed issued-at values, fractional timestamp boundaries,
and incorrect EC curve metadata. The final verifier checks registered claims
after signature verification using raw numbers and a deterministic test clock;
ES256 additionally requires P-256 metadata. The audience-array membership check
matches jose, including mixed arrays containing the expected string. The current focused suite also covers missing and mistyped
claims, both signing algorithms, untrusted issuer/audience, algorithm confusion,
key metadata, critical headers, exact payload limits, cache TTL and rollover,
failed refreshes, multiple issuers, and concurrent single-fetch behavior.

The coordinator independently ran the production HTTP JWKS tests, then all
provisional A+D HTTP, JWT, auth configuration and inherited integration tests:
they passed. The tests cover real RSA/EC verification and cache reuse, malformed
upstream responses, signed MCP requests, per-request scope changes, eight scope
listings, prompts, sanitized 401s, argument logging and peer IP. Stable 1.98 Clippy
also passed. The combined candidate on C `d568ce1` subsequently passed stable
Clippy and 83 non-socket unit/auth tests. Full combined HTTP, dependency, semver,
mutation and exact-head review gates remain required.

Public proxy Host tests failed first for a configured nondefault port, then for
an omitted standard HTTPS port. The transport now permits the exact configured
public authority and normalizes its omitted standard port without widening the
allowed host/port set. The default-port regression passed in the coordinator's
HTTP run. That run
exposed a separate logging-test race: thread-local tracing capture missed events
from concurrent server tasks. The test now runs in an isolated subprocess with
one global subscriber while preserving both logging modes, credential-redaction,
identity and peer-IP assertions.

Independent QA then reproduced malformed critical-header acceptance and Unicode
scope-separator differences. Raw signed header tests failed for `crit: []`, and
signed scope tests failed for U+0085 and U+FEFF. The verifier now validates raw
critical-header JSON: nonempty recognized `b64` listings require Boolean true,
including duplicate recognized entries; malformed or unknown critical names fail.
Every accepted matrix case also tests a deliberately corrupted signature. String
scope splitting uses the exact ECMAScript whitespace set; arrays retain exact
member semantics. Development fallback behavior has matching regression coverage.

## Implementation and intentional differences

- Object-safe async authentication; no blocking network calls in the request path.
- Per-issuer guarded cache; refresh failure never extends expired trust.
- JWT required `exp`, exact issuer/audience, nonempty subject, `nbf`, algorithm
  and JWK use/key operation validation. All eight scope wire values come from A.
- Reference development fallback is restored for missing or malformed headers.
- Client auth failures use fixed messages. Authentication tracing excludes
  bearer contents and token IDs. Argument logging is opt-in and redacts known
  credential fields; private note content remains sensitive when opted in.
- Required expiry differs deliberately from jose's optional-exp reference and
  fulfills adopted issue #38. Bounded network/cache behavior is security hardening.
- The TLS verifier includes Mozilla trust-root data from `webpki-root-certs`.
  Its CDLA-Permissive-2.0 license has a package-specific cargo-deny allowance;
  the license text is retained in `docs/licenses/webpki-root-certs.txt`.
- AWS-LC provides cryptography, avoiding the currently unpatched RustCrypto RSA
  advisory rather than accepting an advisory waiver.

## Mutation evidence

The first JWT pass tested 56 mutants: 30 caught, 6 unviable, 20 missed. Those
misses identified missing payload-boundary, critical-header, trust-configuration,
and actual HTTP-client coverage. After adding those tests, the second JWT pass
reported 49 caught, 6 unviable and one survivor. That survivor combined a scheme
and missing-host condition; a separate FTP-with-host regression now exercises it.

The focused auth configuration pass tested 16 mutants: 14 caught, 2 unviable,
none missed. The initial tool argument pass had 5 caught and 5 missed; a new
validator table test covers empty/missing/non-string values, nonempty strings,
both Boolean values, and omitted/invalid optional Booleans. The first combined
stable-toolchain attempt stopped at its unmutated baseline because of the logging
test race; it ran no mutants. The corrected combined stable-toolchain pass
completed 100 cases: **83 caught, 17 unviable, zero missed**. It included all auth HTTP tests without skips, auth
modules, selected auth configuration validators and tool argument helpers. The
unviable cases did not compile; they are not counted as caught tests. This run
covers provisional A+D head `7a691a50c95174cfbf33efd517e90a806d5243ac`,
not the final combined A/B/C/D head.

The fresh integrated auth/configuration/argument run at `e8743e2` tested **128
mutants: 111 caught, 17 unviable, zero missed or timed out**, after its unmutated
baseline passed. It covers the corrected signed claims, critical headers and
ECMAScript scope parsing. Final transport-only integration must preserve the
mutated source files and pass the complete unmutated suite; the final review
records that comparison separately.

## Public API compatibility

The 0.4 → 0.5 migration changes `Authenticator::authenticate` from a synchronous
`Result` to public `AuthFuture`, a boxed `Send` future borrowing the authenticator
and authorization header. Implementers change their return type; callers await
the result. This is an intentional pre-1.0 minor source break.

The forced-patch semver diagnostic reports `trait_newly_sealed` (222 checks pass,
one fails, 31 skip). An independent downstream crate successfully implemented
the trait through public `AuthFuture` and used `Arc<dyn Authenticator>`, so the
trait remains externally implementable. The diagnostic is recorded rather than
misreported as a passing compatibility check.

## Verification status

Draft [PR #52](https://github.com/pullupandcode/second-brain-rs/pull/52).
Work in progress. Final stable Clippy, complete integrated tests, deny/audit,
semver classification, mutation verification after integration, independent
code/QA approvals,
PR and release links will be recorded before closure. No full-parity or release
approval is asserted by this report yet.

# Parity delivery ledger

Reference: second-brain-mcp v1.1.1 at
`48b272337a6ef7e381fd175b2ff1844c02cebd7a`.
See the [scope and gates](PARITY_PLAN.md). Status snapshot: 2026-09-16.

All four implementation PRs are open. No parity implementation has been merged
or released. The preexisting immutable [v0.1.0](https://github.com/pullupandcode/second-brain-rs/releases/tag/v0.1.0)
remains unchanged.

| Scope / version | Current status | PR | Independent verification | Merge / release |
|---|---|---|---|---|
| A: protocol, reads, skills, OCR / 0.2.0 | Shared numeric correction integrated; final checks and reviews pending | [#51](https://github.com/pullupandcode/second-brain-rs/pull/51) | Renewed code and QA review of `6fbdd122` pending | Not merged; 0.2.0 not published |
| B: writes, deletes, audit / 0.3.0 | Shared numeric correction integrated; final checks and reviews pending | [#49](https://github.com/pullupandcode/second-brain-rs/pull/49) | Renewed code and QA review of `834ad9d` pending | Not merged; 0.3.0 not published |
| C: frameworks, records, daily / 0.4.0 | Implemented; `f9e11d05` final checks and reviews in progress | [#50](https://github.com/pullupandcode/second-brain-rs/pull/50) | Final code/QA approvals pending | Not merged; 0.4.0 not published |
| D: JWT, deployment, closure / 0.5.0 | JWT and HTTP auth implemented; focused mutations passed; combined integration prepared; final verification pending | [#52](https://github.com/pullupandcode/second-brain-rs/pull/52) | Final code/QA approvals pending | Not merged; 0.5.0 not published |

Previous A/B approvals were withdrawn after the shared numeric-parser correction
changed their candidate heads. A `6fbdd122`, B `834ad9d` and C `f9e11d05` are now
undergoing renewed exact-head checks and independent reviews. D is correcting
QA findings and awaits final integration, mutation testing and independent
reviews. These are explicitly labeled agent reviews, not separate GitHub accounts.
GitHub ruleset `17368994` additionally requires a formal approval from
another account before merging into main. The coordinator's squash-merge attempt
was rejected by that rule; no protection has been bypassed. A formal reviewer
must also be arranged before release sequencing can begin.

B/C/D candidates are being integrated and checked while that gate is pending.
Each dependent PR must subsequently be based on the actual preceding squash
commits. Any new commit requires both independent reviewers to approve the new
exact head. Green CI, required GitHub reviews and resolved threads are also merge
gates. Approved scopes merge A → B → C → D, followed by matching version tags and
GitHub releases whose `immutable: true` state is verified after publication.

## Evidence and remaining closure

- Pinned TypeScript baseline: 181 tests passed across 21 files.
- Initial Rust baseline: 51 unit tests passed; 11 of 12 integration tests passed.
  The initial empty-query search failure is corrected in A.
- A's previously reviewed head passed format, current-stable Clippy, 77
  unit/integration tests, dependency checks and semver classification. The corrected privacy candidate subsequently passed 56 unit and 23 integration
  tests; its final exact-head review evidence is still being collected.
- D's signed-token tests demonstrated the old JWT-to-development fallback failing
  rejection tests before implementation. Real HTTP tests cover signatures, scopes,
  cache behavior, proxy Host handling, error redaction and logging. The corrected
  HTTP suite passed four consecutive runs after isolating tracing capture.
- The corrected comprehensive D mutation run completed 100 cases: 83 caught,
  17 unviable, zero missed, on the provisional protocol/auth integration. Its
  predecessor stopped at an unmutated test failure and ran zero mutants. The
  combined storage/framework candidate still requires its final integrated checks;
  earlier coverage improvements are recorded in [scope D](parity/auth.md).

Final combined all-feature tests, security/semver checks, mutation outcomes,
exact-head independent approvals, squash SHAs and immutable release URLs remain
to be recorded. There is no full-parity approval yet. Detailed evidence and
intentional reference differences are in [A](parity/A.md), [B](parity/B.md),
[C](parity/C.md), and [D](parity/auth.md).

# PR #49 review disposition

Review: `5226667210`, against the storage implementation; reference:
second-brain-mcp v1.1.1 at `48b272337a6ef7e381fd175b2ff1844c02cebd7a`.
The two inline comments and the summary's additional observations were assessed.

| Review concern | Disposition and evidence |
|---|---|
| Windows rename cannot replace an existing file (inline `4029368996`) | The premise is incorrect for Tokio/Rust. Both APIs document existing-file replacement, including Windows. Retain the safe standard implementation; add Windows CI running actual storage replacement/frontmatter/marker tests. |
| `trash_path = "."` normalizes to the vault root (inline `4029369053`) | Fixed after a failing regression. Validate the normalized path; reject dot/slash/backslash root equivalents while preserving defaults and valid nested directories. |
| Drive-qualified or drive-relative Windows paths | Fixed after a failing regression. Reject ASCII drive prefixes, including prefixes exposed by leading dot-segment removal, on every platform. Ordinary relative paths remain accepted. |
| Replacement widens restrictive Unix file modes | Fixed after a `0600` → `0644` failure. Create replacement temporaries with restricted permissions, restore existing permission bits before publication, and test all three replacement operations with `0600` and `0640`. Extended ACL/ownership preservation is not claimed. |
| Rotation hides incomplete attempts (reported twice) | Fixed after a failing regression. Defer rotation whenever incomplete lifecycle starts exist. They remain queryable in the live store; retention resumes after reconciliation. |
| Archive timestamps can collide | Fixed with exclusive filename reservation and collision suffixes. A deterministic same-timestamp test verifies old archive bytes survive; rename-failure cleanup removes only the new reservation. |
| Completion and provenance are separate commits | Fixed through one transaction used by the writer. Injected failures in either insert roll back the pair. Filesystem success remains success while a recorded pending attempt stays recoverable. |
| Trash is visible without an ignore rule | Matches the reference contract: `docs/user-guide.md:339` instructs operators to exclude trash if desired. The default example ignores `.trash/**`; custom destinations need matching soft exclusions. No unconditional hard privacy rule is added. |
| Configured private skill files are readable as prompts | Matches the separately authorized skill surface. The reference explicitly tests this in `tests/unit/runtime.test.ts:74–126` and documents it in `docs/user-guide.md:214`. Ordinary vault reads remain denied. Clarified the Rust configuration comment. |
| Soft exclusions affect direct reads | Matches reference runtime/reader behavior and `docs/user-guide.md:128`: these are read/list/search exclusions, not mutation prohibitions. Clarified the configuration comment. |
| A soft-excluded conflict is quarantined | The conflict causes quarantine, not the soft exclusion. External probes confirm normal writes under the soft-excluded subtree succeed while conflicted paths are denied both inside and outside it. Exempting hidden conflicts would weaken independent write protection. |
| Relative SQLite filename has an empty parent | False positive. The external runtime probe starts successfully. Rust `create_dir_all` explicitly treats an empty path as success. |
| Quarantine is stale after external conflict changes | Confirmed, explicitly documented startup-snapshot limitation. New or resolved conflicts require restart for write enforcement; refresh of conflict listings alone does not update the writer snapshot. This exceeds the reference runtime's startup write protection but is not a live watcher. |
| Every mutation performs a full index rebuild | Confirmed correctness/performance tradeoff: immediate read-after-write search is serialized and O(vault size). Incremental indexing is not claimed or implemented by this review fix. |
| Soft deletion uses link then unlink | Confirmed cancellation/crash window; destination reservation is no-clobber but the overall move is not atomic. Scope documentation now states both-name outcomes and non-destructive recovery procedure. No automatic destructive recovery is claimed. |
| `path_blocked` omits the requested path | The existing generic message matches reference `src/vault/writer.ts:246` exactly. Keep the stable code/message; do not introduce an unrequested wire difference. |

Primary API references: [Tokio rename](https://docs.rs/tokio/latest/tokio/fs/fn.rename.html),
[Rust rename](https://doc.rust-lang.org/std/fs/fn.rename.html), and
[Rust create_dir_all](https://doc.rust-lang.org/std/fs/fn.create_dir_all.html).

All fixes have failing-then-passing regression evidence. Tests cover normalized
configuration, cross-platform path input, real replacements, audit fault injection,
retention/reconciliation and archive collisions. Final exact-head independent
reviews and CI results are recorded on PR #49; this document is not a merge or
release approval.

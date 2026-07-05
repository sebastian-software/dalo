# V1 Implementation Status

Status: Current as of 2026-06-24  
Scope: RFC 0001, RFC 0002, RFC 0003 implementation review  

This document records what the first V1 release candidate implements, what remains deferred, and where implementation intentionally differs from the RFC wording.

## Implemented in V1

- Rust 2024 CLI/library package with stable toolchain.
- Store initialization under a configurable store path.
- TOML config, state, approvals, and resolved user lock.
- Local private source as a Git repository.
- Team Git sources cloned into the store.
- `git` CLI wrapper for clone, fast-forward pull, dirty checks, and commit identity.
- `target detect`, `target link`, and `target unlink`.
- Supported target registry for Codex, Claude Code, OpenClaw, Hermes, and generic folders.
- Experimental placeholders for Cursor and OpenCode.
- Inventory scanning for `SKILL.md` skill directories.
- Frontmatter parsing for name, id, description, owners, tags, and requires.
- Deterministic resolver for local/team sources.
- Approval gating by skill, source, author, and org.
- Shadowing reported as `unlinked`.
- Local override reporting.
- Directory-level symlink materialization.
- Owned symlink removal for no-longer-desired skills.
- Dirty team source blocking during `sync`.
- Resolved user lock writing after sync.
- Lock drift reporting during `status`.
- Unmanaged target skill discovery.
- Copy-first adoption into the local source.
- Optional `--replace` replacement of adopted target folders with owned symlinks.
- `.local` and explicitly kept unmanaged skill protection.
- Minimal `resolve list`, `resolve adopt`, `resolve keep`, and `resolve remove-owned`.
- `doctor` diagnostics with text and JSON output.
- Read-only diagnostics for store, config, state, lock, Git, GitHub CLI readiness, targets, symlinks, dirty sources, pending approvals, and unmanaged blockers.
- End-to-end local fixtures that do not require network access.
- CI workflow for Linux and macOS.

## Deferred

- External sources declared by trusted sources.
- Catalog source selection and catalog locks.
- Catalog drift reporting.
- Same-catalog required-closure expansion.
- Dependency preflight for `requires`.
- Instruction pack discovery and managed block rendering.
- Scheduled autosync installation.
- `source refresh` lockfile PRs.
- Full interactive resolve assistant.
- Rename/adapt flows for conflicts.
- Full PR-first `promote`.
- GitHub PR creation through `gh`.
- GitLab, Forgejo, and other forge adapters.
- Homebrew tap, Cargo publish, and homepage.
- Windows support.

## Intentional Deviations

- The store lock uses a create-new `.lock` file with retry behavior rather than OS advisory `flock`. This keeps V1 portable and testable, but stale lock takeover is deferred.
- Scheduled sync behavior is not implemented yet, so scheduled lock semantics remain deferred.
- `sync` currently materializes safe slots and reports blocked materialization operations instead of making every target conflict a run-level failure.
- `gh` is checked by `doctor`, but no V1 command creates PRs yet because `promote` is deferred.
- Instruction-related modules from RFC 0002 are not present in V1 because instruction packs moved to V1.1.
- Source removal is not yet a CLI command; orphan cleanup behavior is covered by config/source disappearance and owned symlink reconciliation.

## Versioning Decision

The first releasable build should be tagged as `v0.1.0-rc.1`. The crate version remains `0.1.0` until the first non-RC V1 tag. The RC is intended for local team testing, not package registry publication.

# ADR 0012: Scoped Project Source Checkouts

Status: Accepted
Date: 2026-10-14
Amends: [ADR 0009](0009-explicit-project-scope.md)

## Context

Project declarations pin whole Git commits, but an agent skill can live inside
a large monorepo. The depth-1 checkout in ADR 0009 omits history but still
downloads and stores every file blob. That makes each fresh project or worktree
pay for content it never uses.

## Decision

Project declaration schema version 3 adds the optional `path` field to a
source. An explicit path names a tracked repository directory and is trusted
after validating it as a repository-relative path. `.` and `/` select the
repository root. When `path` is absent, Dalo finds tracked `SKILL.md` files,
ignores any whose path contains a dot-prefixed directory, and selects their
shared parent directory. If no visible `SKILL.md` is found, installation fails
with guidance to set an explicit path. Skill selectors are relative to the
repository root, preserving their existing meaning as the checkout is
narrowed.

Dalo fetches the declared commit with depth 1 and `--filter=blob:none`, reads
its tree metadata, and configures sparse checkout for that directory before it
checks out the pin. File blobs outside the selected directory stay absent. The
commit pin, dirty-check protection, and offline reuse of an unchanged checkout
remain in force. A clean existing checkout is narrowed when its declared scope
changes; a dirty checkout still blocks the operation.

A server that ignores blob filtering is not allowed to leave a full repository
behind: installation fails before publishing the staged checkout. An explicit
repository-root path requests the full working tree. `project add --path`
records the explicit directory and upgrades the declaration to schema version
3; without that option, it leaves automatic detection enabled.

## Consequences

- Fresh worktrees retain the pinned commit and tree metadata plus the selected
  skill directory, rather than every blob in a monorepo.
- The Git server must support both shallow fetches of the exact commit and blob
  filtering. Dalo does not silently degrade to an unfiltered checkout.
- A tracked skill under a dot-folder is excluded from automatic detection but
  remains available through an explicit `path`.
- Schema versions 1 and 2 keep their existing meaning. Dalo versions that do
  not support schema version 3 reject it.

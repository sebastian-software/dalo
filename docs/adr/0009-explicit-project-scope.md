# ADR 0009: Project Discovery and Explicit Scope Overrides

Status: Accepted  
Date: 2026-09-26  
Related: [Issue 851](https://github.com/sebastian-software/dalo/issues/851)

## Context

A project should describe its skills in version control without committing
machine-specific symlinks or requiring a second skill manager. Existing target
path overrides are insufficient: targets and their resolved skill set belong
to one store, and switching them would affect global installations.

## Decision

Extend ADR 0002 with independent project stores. With no explicit CLI or
environment store override, discover the nearest `dalo-project.toml` from the
current directory upward, stopping at the nearest `.git` boundary. Invalid
project declarations fail closed. A new project declaration opts a repository
into this behavior; repositories without it retain the existing store default.

`--global`/`-g` selects `~/.dalo` even when `DALO_STORE` is set. `--project`
selects an exact directory; `--store` and `DALO_STORE` bypass discovery. The CLI
scope flags conflict. Store-independent commands do not use automatic discovery.
An interactive `init` in an unconfigured Git repository asks for project versus
global scope, with cancellation as the default. Scripts, JSON, dry-run, and CI
never prompt. No existing persisted schema changes.

The portable declaration is `dalo-project.toml`, with its own schema version.
Do not overload the existing team-source `dalo.toml`. Declarations cannot grant
trust or choose arbitrary output paths. Built-in target IDs map to bounded
project folders; redirected folders and unmanaged entries block writes.

The first increment requires full Git commit IDs and explicit skill selections.
It reuses untrusted catalogs, local content-bound approvals, deterministic
resolution, and safe materialization. Local checkouts, receipts, approvals,
locks, and generated links remain untracked. Later ref resolution needs a
separate portable lockfile and an explicit update operation; installation must
never silently advance a pin.

## Failure and recovery

Validate the declaration and existing store before preparation. Serialize
preparation and delivery with the store lock. Do not adopt an existing `.dalo`
without the project ownership receipt. A partial preparation can retain
registered sources for review and retry. Unexpected checkout remnants, dirty
sources, pin changes, and changed selections fail closed. The first increment
does not promise an atomic rollback of all fetched sources.

## Evidence

CLI tests use real local Git repositories and isolated homes to verify scope discovery and overrides, exact-commit restoration into two projects, local approval, idempotence,
preview behavior, preservation of edits, and rejection of redirected paths.

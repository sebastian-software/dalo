# ADR 0003: Rust CLI That Shells Out to Git

Status: Accepted  
Date: 2026-09-06  
Source: [RFC 0002: Technical Architecture](../rfcs/0002-technical-architecture.md)  

## Context

Dalo touches home directories, Git checkouts, symlinks, lockfiles, scheduler
files, and agent instruction files. The dominant risks are data loss, ambiguous
state transitions, and weak error reporting. Installation has to stay trivial on
developer machines.

## Decision

Dalo is a Rust program distributed as a single `dalo` binary for macOS and
Linux.

- Stable Rust only, Rust 2024 edition, pinned through `rust-toolchain.toml`.
- No Node, Python, or long-running runtime at execution time.
- Git operations shell out to the installed `git` CLI, and GitHub operations to
  `gh`, instead of embedding a Git implementation or handling credentials.
- Configuration and lockfiles are TOML; machine-readable command output is JSON.
- A small, audited dependency set.

## Consequences

- The user's existing Git and GitHub authentication keeps working, and Dalo
  never stores credentials.
- `git` (and `gh` for promotion flows) must be on `PATH`; their absence is a
  diagnosable state rather than a fallback path.
- Typed domain models make source, target, skill, instruction, and lock states
  explicit, and tests can drive real temporary directories and repositories.
- Windows is out of scope until a separate decision changes it.

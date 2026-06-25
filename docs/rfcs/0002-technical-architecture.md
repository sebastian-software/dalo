# RFC 0002: Technical Architecture

Status: Draft  
Date: 2026-06-23  
Author: Sebastian + Codex  
Depends on: RFC 0001

## 1. Summary

Dalo should be implemented as a Rust CLI distributed as a single `dalo` binary. The v1 implementation should use the existing `git` and `gh` CLIs for Git and GitHub operations instead of embedding a Git implementation or managing credentials directly.

This architecture optimizes for predictable local behavior, strong state modeling, careful filesystem operations, and simple installation on macOS and Linux.

## 2. Core Decisions

- Language: Rust.
- Edition: Rust 2024.
- Toolchain: stable Rust only; no nightly-only features.
- Packaging: one binary named `dalo`.
- Runtime dependencies: no Node, Python, or long-running runtime required.
- Git operations in v1: shell out to `git`.
- GitHub PR operations in v1: shell out to `gh`.
- Config and lockfiles: TOML.
- Machine-readable command output: JSON.
- Supported platforms: macOS and Linux.
- Windows: out of scope for this RFC, matching RFC 0001.

The local development machine currently has a nightly Rust toolchain installed, but the project should not depend on nightly. The first implementation should add a stable `rust-toolchain.toml`.

## 3. Why Rust

Dalo is a local state-management CLI that touches user home directories, Git checkouts, symlinks, lockfiles, scheduler files, and agent instruction files. The main engineering risks are accidental data loss, ambiguous state transitions, path handling mistakes, and weak error reporting.

Rust is a good fit because:

- typed domain models help keep source, target, skill, instruction, and lock states explicit
- filesystem and process boundaries can be wrapped behind narrow interfaces
- a single static-ish binary is easy to distribute
- CLI and serialization ecosystem support is mature
- tests can exercise real temporary directories and Git repositories without a runtime service

Go would also be viable, especially for simple binary distribution. Rust is preferred because the resolver, lockfile, and materialization logic will benefit from stronger type modeling and explicit error handling.

## 4. Dependency Strategy

Use a small dependency set in v1:

- `clap`: CLI parsing and help text
- `serde`: typed serialization and deserialization
- `toml`: TOML config and lockfile parsing/writing
- `serde_json`: `--json` output
- `serde_yaml`: YAML front-matter parsing in the skill inventory
- `sha2`: content fingerprints for skill files in the inventory
- `thiserror`: typed library errors
- `anyhow`: CLI boundary error context
- `tempfile`: tests and safe temporary writes

Likely test-only dependencies:

- `assert_cmd`: command-level CLI tests
- `predicates`: CLI output assertions
- `insta`: snapshots for stable JSON/TOML/status output, if useful

Avoid in v1:

- `libgit2`/`git2`, because native dependencies and credential behavior add complexity
- embedded Git mutation through `gix`/gitoxide, though it remains a future option
- async runtime unless a later feature genuinely needs concurrency
- TUI frameworks
- daemon frameworks

The implementation should prefer the standard library for path, process, and filesystem work unless a dependency removes real risk.

## 5. Git and GitHub Integration

V1 should treat Git as an external tool controlled through a narrow wrapper.

Rules:

- invoke `git` with explicit arguments, never through shell string concatenation
- always set the working directory explicitly
- parse stable machine-oriented output where available
- use `git status --porcelain=v2` for dirty checks
- use `git rev-parse HEAD` for commit identity
- use `git fetch` and explicit merge/pull behavior rather than broad implicit magic
- surface failed Git commands with command, exit status, stderr, and safe context
- do not store credentials

GitHub PR flows should use `gh` in the same style:

- detect `gh` availability in `doctor`
- use existing `gh` authentication
- create PRs only from explicit `promote` flows in V1
- fail the PR flow when `gh` is missing or not authenticated
- do not create an internal GitHub API client or manage GitHub secrets in V1
- treat GitLab, Forgejo, and other forges as future features rather than V1 adapters

Pure-Rust Git should remain a future optimization. `gix`/gitoxide can be evaluated later for read-only inventory, status, or performance-sensitive operations, but v1 should not depend on it for correctness.

## 6. Project Shape

Start as one Cargo package with a binary and a library:

```text
src/
  main.rs
  lib.rs
  cli.rs
  config.rs
  lockfile.rs
  store.rs
  source.rs
  inventory.rs
  resolver.rs
  materialize.rs
  target.rs
  instruction.rs
  git.rs
  github.rs
  status.rs
  doctor.rs
  error.rs
```

Responsibilities:

- `main.rs`: process entrypoint and top-level exit handling
- `cli.rs`: `clap` command definitions, flags, and output-mode selection
- `config.rs`: user config schema and validation
- `lockfile.rs`: source locks and resolved user lock schema
- `store.rs`: `~/.dalo` layout, state files, atomic writes
- `source.rs`: local, team, external, and catalog source definitions
- `inventory.rs`: scan skills and instruction packs
- `resolver.rs`: priority, shadowing, catalog selection, instruction selection
- `materialize.rs`: symlink creation/removal and dry-run operation plans
- `target.rs`: agent target registry and detection
- `instruction.rs`: managed instruction block parsing/rendering
- `git.rs`: `git` CLI wrapper
- `github.rs`: `gh` CLI wrapper
- `status.rs`: status model and text/JSON rendering data
- `doctor.rs`: diagnostics
- `error.rs`: typed error model

The CLI should call into library operations. Core behavior should be testable without spawning the binary.

## 7. Data and State Model

Use typed Rust structs for all public schemas:

- user config
- source manifest
- source lock
- resolved user lock
- local approval state
- state file
- inventory snapshot
- target registry entries
- status output
- doctor diagnostics

Schema rules:

- every persisted file has a `version`
- parsers reject unknown required sections but tolerate unknown future fields where safe
- JSON output is treated as public API once released
- text output may evolve more freely
- state files are internal and may evolve with migrations

TOML should be used for human-authored config and locks. JSON should be used only for CLI output and machine integrations.

## 8. Filesystem Safety

Filesystem writes must be modeled as explicit operation plans before execution.

Required behavior:

- `--dry-run` prints the planned operations and writes nothing
- materialize each managed skill as one directory-level symlink per target slot
- write generated files through a temporary file plus atomic rename where possible
- never replace a real directory with a symlink without explicit confirmation
- never delete unmanaged files
- only remove symlinks recorded as dalo-owned
- only edit text inside managed instruction block markers
- block on malformed instruction block markers
- preserve unmarked content in instruction files byte-for-byte where possible
- canonicalize paths before checking whether a target points inside the store

The materializer should be split into two steps:

1. build a plan from resolved state and current filesystem state
2. apply the plan

This keeps dry-runs, tests, and user prompts aligned.

## 9. CLI UX and Output

The CLI should default to human-readable text and provide `--json` on commands that expose state.

Global flags:

- `--store <path>`
- `--json`
- `--yes`
- `--dry-run`
- `--no-color`
- `--verbose`

Exit code policy:

- `0`: success
- `1`: expected user/actionable failure
- `2`: invalid CLI usage
- `3`: unsafe state blocked the requested operation
- `4`: dependency or environment problem

Error messages should include:

- what failed
- why dalo refused or stopped
- the next command to inspect or resolve the issue

## 10. Scheduler Strategy

Scheduled sync should not require a daemon in v1.

Target model:

- macOS: generate a launchd plist that runs `dalo sync --yes --quiet`
- Linux: generate a systemd user timer and service
- cron fallback only if systemd user timers are unavailable

V1 does not need full scheduler installation. The architecture should still keep scheduler generation isolated so it can be added without changing sync behavior.

## 11. Testing Strategy

Testing should focus on filesystem safety and deterministic resolution.

Test layers:

- unit tests for config parsing, locks, resolver, instruction block parsing, and path validation
- integration tests using temporary stores and targets
- Git integration tests using local bare repositories and working clones
- snapshot tests for stable `--json` output and selected text output
- dry-run tests that assert no filesystem mutation

High-value scenarios (these describe the full vision; items marked (V1.1) ship with the catalog/instruction-pack slice deferred in RFC 0001 §26):

- existing unmanaged skill is detected and not modified
- managed skill colliding with an unmanaged target entry reports a conflict and leaves the unmanaged entry untouched
- logical targets that resolve to the same canonical directory are de-duplicated before materialization while still reported as separate logical agent targets
- equal slot names resolve by priority and report non-winning skills as unlinked with reason `shadowed`
- newly active unapproved skill is reported and not materialized by scheduled or non-interactive sync
- source-, author-, and org-level approvals allow matching newly active skills without per-skill prompts
- catalog source selects only configured skills (V1.1)
- same-catalog required dependencies are expanded transitively before materialization (V1.1)
- selected skill is blocked before materialization when a required dependency is missing, unapproved, shadowed by a non-equivalent winner, or blocked by a same-name target entry (V1.1)
- removed selected catalog skill blocks scheduled sync (V1.1)
- instruction block rendering preserves unmarked content (V1.1)
- malformed instruction block markers block writes (V1.1)
- instruction pack rendering does not depend on native include/import support (V1.1)
- dirty Git source blocks scheduled or non-interactive sync
- symlink removal only affects dalo-owned links

Tests should not require network access.

## 12. Distribution

Initial distribution can be:

- `cargo install --path .` during development
- GitHub release binaries later
- Homebrew tap later
- cargo-binstall support later if useful

The binary should not assume it was installed by Cargo. It should locate its store through explicit `--store`, environment override, or the default `~/.dalo`.

## 13. Deferred Choices

These are intentionally not part of v1:

- embedded Git mutations with `gix` or `git2`
- a background daemon
- TUI
- desktop UI
- plugin system
- remote service
- Windows path and symlink policy
- semantic conflict detection between instruction packs

## 14. Acceptance Criteria

- The repository is scaffolded as a stable Rust 2024 project with one `dalo` binary and a reusable library.
- Core behavior is reachable through library functions, not only CLI handlers.
- Git and GitHub operations go through narrow wrappers around `git` and `gh`.
- Config, locks, approvals, inventory, status, and doctor data use typed structs.
- `--dry-run` uses the same operation planning path as real execution.
- Filesystem mutation code cannot delete unmanaged files by default.
- Instruction pack rendering edits only managed blocks and does not rely on native include/import support (V1.1).
- Tests cover resolver behavior, approval gating, Git dirty detection, and symlink ownership; catalog selection and instruction block rendering tests ship with V1.1 (RFC 0001 §26).
- The project builds and tests on macOS and Linux without network access.

# ADR 0008: Compatibility Contract and Library API Stance

Status: Accepted  
Date: 2026-09-17  
Source: [Issue 802](https://github.com/sebastian-software/dalo/issues/802), [Issue 805](https://github.com/sebastian-software/dalo/issues/805)  

## Context

Dalo documents its commands, exit codes, `--json` shapes, and persisted schemas
precisely, but has never said which of them are promises. Someone scripting
`dalo status --json` in CI, committing `dalo.toml` for a team, or keeping
`~/.dalo` for months cannot tell whether an upgrade inside a major version is
safe. `SECURITY.md` supports "the latest release" without saying what happens to
a 1.x line.

At the same time, `cargo add dalo` works and the crate renders 34 public modules
on docs.rs. A 1.0 on crates.io conventionally implies semver for that surface.
The project has never treated the library as a contract, and the tension already
shows in commits that preserve an internal API because a test depended on it.
Publishing a curated `dalo-core` would be the alternative, but no second
consumer is planned for 1.x, so the cost would buy nothing.

Both questions have to be answered together: the library stance is one row in
the compatibility table.

## Decision

Dalo publishes a written compatibility contract in
[`docs/compatibility.md`](../compatibility.md), rendered on dalo.sh, and it is
binding for the 1.x line.

- Surfaces are sorted into three tiers. **Stable in 1.x**: the commands and
  flags in the reference, the exit codes `0`–`4`, the top-level `--json` report
  shapes, the persisted files (`config.toml`, `state.toml`, `lock.toml`,
  `approvals.toml`, `source-lock.toml`, team `dalo.toml`, `PLUGIN.toml`, plus
  `DELIVERY.toml` and `AGENT.md`), the documented environment variables, and the
  named install channels. **Experimental**: the Portable Agent Packages draft,
  provider plugin and hook projections, and any target whose reported `support`
  is `experimental`. **Not covered**: human-readable output text, store-internal
  layout, and the Rust library API.
- The change policy is fixed: breaking changes only in a major; deprecations
  announced at least one minor ahead with a runtime warning on stderr that never
  changes the exit status and is suppressed under `--json`; JSON fields never
  removed within 1.x; schema bumps migrate forward automatically and fail closed
  on downgrade rather than rewriting a file at an older version.
- Supported platforms are macOS and Linux on `x86_64` and `aarch64`, with both
  `gnu` and `musl` for Linux. Windows is supported through WSL only; there is no
  native Windows build in 1.x, which the crate already enforces with a
  `compile_error!` on non-Unix targets.
- Security fixes land in the latest 1.x release; older 1.x releases are not
  patched separately. After 2.0, the 1.x line receives security fixes for six
  months.
- The Rust library API is not a semver contract; the CLI, its exit codes, its
  `--json` output, and the files Dalo persists are. The crate stays a single
  crate, the statement is the first paragraph of the `src/lib.rs` crate
  documentation so docs.rs shows it first, and modules that are pure CLI
  plumbing (`cli`, `term`, `update`) are `#[doc(hidden)]`. No `dalo-core` split
  is made; it would be reconsidered only when a second real consumer exists.

The alternatives considered and rejected were: leaving stability implicit, which
is what produced the question; promising semver for the library, which would
freeze internal refactoring for a surface nobody consumes; and splitting
`dalo-core` now, which is real work with no current beneficiary.

## Consequences

- Any change to a tier-1 surface is now a reviewable question with a documented
  answer, and reviewers can point at a tier instead of arguing from taste.
- Integrations are steered to `--json` and exit codes, so text output stays free
  to improve and internal modules stay free to change.
- `tests/docs.sh` enforces that the page exists, names every persisted file, and
  carries the library statement verbatim alongside `src/lib.rs`, so the contract
  cannot drift away from the code silently.
- Experimental surfaces must stay labeled where they appear, not only on the
  compatibility page. Adding an experimental command or target now carries that
  obligation.
- The three-tier split is a constraint on future work: promoting a surface from
  experimental to stable is a deliberate decision, and a 2.0 is the only place
  where a tier-1 removal can land.

# Dalo Implementation Status Snapshot

**Archived.** This snapshot is frozen at the date below and is no longer
maintained. It lived at `docs/rfcs/v1-implementation-status.md` and is kept only
so links and RFC readers still land on something. For what actually ships today,
read [`CHANGELOG.md`](../../CHANGELOG.md) and
[`docs/reference.md`](../reference.md); for what is still planned, read epic
[#836](https://github.com/sebastian-software/dalo/issues/836), where every item
of the "Still Planned" list below is now an open issue you can subscribe to,
react to, and comment on. Nothing here is a commitment.

Status: Superseded by `CHANGELOG.md` and `docs/archive/milestones/README.md` for release-by-release tracking
Last updated: 2026-07-05
Current crate version: see `Cargo.toml`

This document is a compact status snapshot for readers coming from the V1 RFCs. It no longer tries to duplicate every shipped command or every changelog entry. For exact release history, use the changelog. For milestone acceptance criteria, use the milestone index and individual milestone files.

## Shipped Through the Current Release

The V1 local/team skill loop is implemented:

- Rust 2024 CLI/library package.
- Store initialization under a configurable store path.
- TOML config, state, approvals, source lock, and resolved user lock.
- Local private source as a Git repository.
- Team Git sources cloned into the store, with clean tracking refresh and dirty-source blocking.
- `target detect`, `target link`, and `target unlink`.
- Supported directory targets for Codex, Claude Code, OpenClaw, Hermes, and generic folders; Cursor and OpenCode remain experimental placeholders.
- Inventory scanning for portable `SKILL.md` skill directories.
- Frontmatter parsing for `id`, `name`, `description`, `owners`, `tags`, and `requires`.
- Deterministic resolver with priority, approvals, shadowing, local override reporting, and required-closure checks.
- Directory-level symlink materialization that only removes dalo-owned links.
- `status`, `sync`, `adopt`, minimal `resolve` helpers, and `doctor` diagnostics.
- Linux/macOS CI, MSRV checks, dependency audit, and coverage summary.

The V1.1 catalog and instruction-pack layer is also implemented:

- Catalog sources via `source add-catalog`, `source inspect`, and `source select`.
- Source locks for pinned catalog commits, selected skills, and inventory snapshots.
- Read-only catalog drift reporting through `source refresh --check`.
- Same-catalog required-closure expansion with approval and linkability preflight.
- Instruction packs rendered into isolated managed blocks through `instructions enable` and removed through `instructions disable`.
- Instruction pack discovery, `instructions list`, and topic-overlap warnings in `status` and `doctor`.
- Team-owned `dalo.toml` composition of pinned external catalogs, including
  include/exclude skill filters and local approval gating.
- Store-independent `dalo team` management commands for authoring and updating
  the versioned team manifest.
- Reviewed team catalog pin updates that resolve an upstream ref, preview
  inventory drift and deterministic audits, and write only the exact commit.
- Recurring background `sync` via `autosync install` on launchd, systemd user
  timers, or a marked crontab entry, with last-run status in `status`/`doctor`.
- A pre-link security layer: deterministic `audit` preflights, optional
  agent-assisted review (`--reviewer`), and per-skill/source `approve` /
  `approve revoke` trust records.

Distribution work is wired for the next tagged release:

- Root `LICENSE-MIT` and `LICENSE-APACHE` (the crate is `MIT OR Apache-2.0`).
- Release workflow publishes to crates.io when release-please creates a release, assuming `CARGO_REGISTRY_TOKEN` is configured.
- Release workflow attaches Linux and macOS archives plus SHA-256 checksum files to GitHub releases.

## Still Planned

Each entry is an open issue under epic
[#836](https://github.com/sebastian-software/dalo/issues/836). The issue is the
live record; this list is the frozen prose it replaced.

- Non-catalog external sources with subpath scoping — [#853](https://github.com/sebastian-software/dalo/issues/853).
- Lock-advancing `source refresh` that opens lockfile PRs (advancing a catalog's own pin already ships via `source refresh --advance`) — [#829](https://github.com/sebastian-software/dalo/issues/829).
- Full interactive resolve assistant — [#833](https://github.com/sebastian-software/dalo/issues/833).
- Rename/adapt flows for conflicts — [#833](https://github.com/sebastian-software/dalo/issues/833).
- Full PR-first `promote` — [#828](https://github.com/sebastian-software/dalo/issues/828).
- Forge adapters beyond GitHub — [#854](https://github.com/sebastian-software/dalo/issues/854).
- More verified target adapters beyond the current supported set — [#831](https://github.com/sebastian-software/dalo/issues/831).
- Windows support — [#830](https://github.com/sebastian-software/dalo/issues/830).

## Intentional Deviations From Early RFC Text

- Native include/import support for instruction files is not the baseline because it is not portable across agents. Dalo uses explicit managed blocks instead.
- Cross-source `requires` are checked and reported, but are not auto-installed across source boundaries.
- Catalog drift checking through `source refresh --check` is read-only; advancing a catalog pin ships separately as the explicit `source refresh --advance`.
- `gh` is checked by `doctor`, but no shipped command creates PRs yet because `promote` remains planned.

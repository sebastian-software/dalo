# Dalo Implementation Status Snapshot

Status: Superseded by `CHANGELOG.md` and `docs/milestones/README.md` for release-by-release tracking
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

Distribution work is wired for the next tagged release:

- Root MIT `LICENSE`.
- Release workflow publishes to crates.io when release-please creates a release, assuming `CARGO_REGISTRY_TOKEN` is configured.
- Release workflow attaches Linux and macOS archives plus SHA-256 checksum files to GitHub releases.

## Still Planned

- Non-catalog external sources with subpath scoping.
- Lock-advancing `source refresh` that opens lockfile PRs or updates pins.
- Full interactive resolve assistant.
- Rename/adapt flows for conflicts.
- Full PR-first `promote`.
- Forge adapters beyond GitHub.
- More verified target adapters beyond the current supported set.
- Homebrew tap and other package-manager integrations.
- Windows support.

## Intentional Deviations From Early RFC Text

- Native include/import support for instruction files is not the baseline because it is not portable across agents. Dalo uses explicit managed blocks instead.
- Cross-source `requires` are checked and reported, but are not auto-installed across source boundaries.
- Catalog drift checking is read-only. Advancing a catalog pin remains a later source-maintenance flow.
- `gh` is checked by `doctor`, but no shipped command creates PRs yet because `promote` remains planned.

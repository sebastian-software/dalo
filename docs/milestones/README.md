# Dalo Implementation Milestones

Status: V1 and V1.1 complete
Scope: V1 implementation plan plus the V1.1 catalog and instruction-pack plan
Last updated: 2026-07-05

This directory turns the RFCs into implementation milestones. The RFCs remain the product and architecture contract; these files define the order of work, acceptance criteria, and validation gates.

Milestones are intentionally small and stable. Progress is tracked by changing the `Status:` field in each milestone file:

- `todo`: not started
- `in_progress`: actively being implemented
- `blocked`: cannot move without a decision or dependency
- `done`: implemented, validated, and committed

## Milestone Record

| ID | Milestone | Status | Purpose |
| --- | --- | --- | --- |
| M00 | [Repository and Toolchain Scaffold](0000-repository-toolchain-scaffold.md) | done | Create the Rust project shape and baseline validation loop. |
| M01 | [Store, Config, and Init](0001-store-config-init.md) | done | Establish `~/.dalo`, TOML schemas, local source, and `init`. |
| M02 | [Target Registry](0002-target-registry.md) | done | Detect and link supported agent skill directories safely. |
| M03 | [Inventory Scanner](0003-inventory-scanner.md) | done | Scan local/team checkouts into deterministic skill inventories. |
| M04 | [Resolver and Status Model](0004-resolver-status-model.md) | done | Resolve source priority, approvals, shadowing, and status output. |
| M05 | [Materializer and Sync Loop](0005-materializer-sync-loop.md) | done | Plan/apply directory symlinks without touching unmanaged files. |
| M06 | [Team Sources and Git Safety](0006-team-sources-git-safety.md) | done | Add team Git checkouts, dirty checks, and clean tracking refresh. |
| M07 | [User Lock and Multi-Source Reproducibility](0007-user-lock-multisource.md) | done | Persist the resolved set and make multi-source sync reproducible. |
| M08 | [Adopt and Minimal Resolve](0008-adopt-minimal-resolve.md) | done | Bring unmanaged skills into the local source and expose safe repair commands. |
| M09 | [Doctor and Diagnostics](0009-doctor-diagnostics.md) | done | Provide environment, path, Git, target, and state diagnostics. |
| M10 | [V1 Release Readiness](0010-v1-release-readiness.md) | done | Harden CLI UX, docs, tests, packaging, and release gates. |
| M11 | [Catalog Source and Selection](0011-catalog-source-selection.md) | done | Inspect multi-skill catalog repos, select skills, pin the catalog commit and selection. |
| M12 | [Catalog Drift Reporting](0012-catalog-drift-reporting.md) | done | Detect new/changed/moved/removed catalog skills before changing the resolved set. |
| M13 | [Same-Catalog Required-Closure](0013-required-closure-expansion.md) | done | Expand `requires` within a source/catalog, with approval and linkability preflight. |
| M14 | [Instruction Pack Managed Blocks](0014-instruction-pack-managed-blocks.md) | done | Render versioned instruction packs into isolated managed blocks in agent files. |
| M15 | [Instruction Pack Discovery and Overlap](0015-instruction-pack-discovery-overlap.md) | done | Discover packs across sources and warn on topic overlap between active packs. |
| M16 | [V1.1 Release Readiness](0016-v1-1-release-readiness.md) | done | Harden the V1.1 surface, docs, and tests, and reconnect prepared fields. |

## Recommended Order

The order is deliberate:

1. Build the Rust/library foundation before feature work.
2. Prove local store and target safety before touching team Git sources.
3. Implement scan -> resolve -> materialize with one source before enabling multi-source complexity.
4. Add Git refresh, locks, and dirty blocking only once local sync is testable.
5. Add adoption and repair commands after ownership tracking exists.
6. Finish with `doctor`, release checks, and documentation.

## V1 Boundary

V1 includes the core local/team skill loop:

- Rust CLI and reusable library
- TOML config, state, approvals, and user lock
- local store under `~/.dalo`
- `init`
- `target detect/link/unlink`
- `source add/list/priority` for local and team sources
- inventory scan for skills with `SKILL.md`
- deterministic resolver
- `status`
- `sync`
- directory-level symlink materialization
- local approval state for newly active team skills
- dirty-source blocking for non-interactive sync
- `adopt`
- minimal `resolve`
- `doctor`

## V1.1 Boundary

V1.1 added the next product layer in M11–M16, in two tracks plus a readiness pass:

- Catalogs (M11–M13): treat multi-skill repositories as offer surfaces — inspect, select, pin, detect drift, and expand same-catalog required closures.
- Instruction packs (M14–M15): render versioned instruction packs into isolated managed blocks, discover packs across sources, and warn on topic overlap.
- Readiness (M16): harden the V1.1 surface and reconnect the fields V1 left in place.

## Deferred Boundary

Later work remains outside this milestone set unless explicitly pulled forward:

- `source refresh` lockfile PRs
- scheduled autosync installer
- full interactive resolve assistant
- rename/adapt flows for conflicts
- full PR-first `promote`
- verified Cursor/OpenCode adapters
- forge adapters beyond GitHub

## Validation Policy

Every implementation PR should run the narrowest useful validation for the milestone, plus:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

Once command-level behavior exists, milestone PRs should also include command tests for the changed CLI surface. Tests must not require network access.

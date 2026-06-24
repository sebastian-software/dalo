# M04: Resolver and Status Model

Status: done
Target: V1  
Depends on: M03, RFC 0003  

## Goal

Implement the pure resolver and status model for local/team sources before any filesystem materialization depends on it.

## Deliverables

- Resolver input model:
  - enabled sources
  - source priority
  - inventories
  - local approval state
  - previous user lock when available
- Resolver output model:
  - active skills
  - pending approvals
  - unlinked skills with reason `shadowed`
  - local override diagnostics
  - blocking diagnostics
- Deterministic conflict handling by slot name.
- Approval matching for skill-, source-, author-, and org-level approvals.
- Text and JSON status rendering data.
- `skillmgr status` for store, source, target, and resolution summaries.

## Out of Scope

- Catalog selection.
- Same-catalog dependency expansion.
- Instruction pack resolution.
- Materialization.

## Acceptance Criteria

- Equal slot names resolve by priority.
- Equal priority numbers tie-break by source ID.
- A local winner over a team skill is reported as a local override.
- An unapproved would-be winner is pending and not active.
- An approved lower-priority skill remains active when a higher-priority candidate is unapproved.
- JSON output is stable enough for snapshot tests.

## Validation

```sh
cargo fmt --check
cargo test resolver status
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/skillmgr-test status --json
git diff --check
```

## Completion Notes

- Added a pure resolver with enabled source selection, deterministic priority ordering, source-ID tie breaks, approval gating, local overrides, pending approval output, and unlinked shadowed skills.
- Added local approval matching for `skill`, `source`, `author`, and `org` scopes.
- Implemented the rule that an unapproved higher-priority candidate does not displace an approved lower-priority skill.
- Added `skillmgr status` with text and JSON output.
- Added status store integration that reads config and approvals, scans enabled source inventories, carries inventory warnings, and renders resolution output.
- Added unit tests for priority, tie-breaks, local override, pending approval, lower-priority approved fallback, and owner-based approvals.
- Added command-level status JSON coverage.
- Validation passed on 2026-06-24.

## Suggested Issue Split

- Implement resolver domain types.
- Implement priority/shadowing algorithm.
- Implement approval matching.
- Implement status text output.
- Implement status JSON snapshots.

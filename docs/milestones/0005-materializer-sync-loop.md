# M05: Materializer and Sync Loop

Status: done
Target: V1  
Depends on: M02, M04, RFC 0002, RFC 0003  

## Goal

Implement the first safe end-to-end local sync loop: inventory -> resolve -> plan -> materialize directory-level symlinks -> record state.

## Deliverables

- Materialization state schema for owned target slots.
- Reconciliation plan model:
  - create owned symlink
  - relink owned symlink
  - recreate missing owned symlink
  - remove orphaned owned symlink
  - drop stale record
  - conflict on unmanaged real entry
  - conflict on foreign symlink
  - no-op
- `dalo sync` for local source and configured targets.
- Directory-level symlink creation.
- `--dry-run` using the same plan path as real sync.
- Idempotent second sync.
- Per-slot conflict reporting while materializing safe slots.

## Out of Scope

- Team Git refresh.
- Dirty Git blocking.
- Scheduler behavior.
- Instruction block rendering.

## Acceptance Criteria

- Managed skills are linked as directory symlinks to store paths.
- Existing unmanaged real directories are never replaced.
- Foreign symlinks are never overwritten.
- User-deleted owned symlinks are recreated.
- Stale owned symlinks are relinked.
- Removed desired skills remove only dalo-owned symlinks.
- Running `sync` twice with no changes produces an all-no-op plan.
- `sync --dry-run` mutates nothing.

## Validation

```sh
cargo fmt --check
cargo test materialize
cargo test sync
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

## Completion Notes

- Added a materialization reconciliation plan for create, relink, remove, drop-record, conflict, and no-op operations.
- Implemented `dalo sync` for the current local/team resolution model without source refresh.
- Materialized managed skills as directory-level symlinks into configured target directories.
- Made `sync --dry-run` use the same plan path while writing nothing.
- Blocked unmanaged real entries and foreign symlinks instead of overwriting them.
- Recorded owned symlinks in `state.toml` and made repeated sync idempotent.
- Added unit tests for symlink creation, unmanaged real-directory conflict, and idempotence.
- Added command-level tests for dry-run, symlink creation, and second-run no-op reporting.
- Validation passed on 2026-06-24.

## Suggested Issue Split

- Implement target slot state schema.
- Implement reconciliation planner.
- Implement symlink apply operations.
- Implement local-source `sync`.
- Add dry-run and idempotence tests.

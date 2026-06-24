# M05: Materializer and Sync Loop

Status: todo  
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
- `skillmgr sync` for local source and configured targets.
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
- Removed desired skills remove only skillmgr-owned symlinks.
- Running `sync` twice with no changes produces an all-no-op plan.
- `sync --dry-run` mutates nothing.

## Validation

```sh
cargo fmt --check
cargo test materialize
cargo test sync
git diff --check
```

## Suggested Issue Split

- Implement target slot state schema.
- Implement reconciliation planner.
- Implement symlink apply operations.
- Implement local-source `sync`.
- Add dry-run and idempotence tests.

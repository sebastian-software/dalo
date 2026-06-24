# M07: User Lock and Multi-Source Reproducibility

Status: todo  
Target: V1  
Depends on: M06, RFC 0001, RFC 0003  

## Goal

Persist the resolved skill set so sync state is reproducible and inspectable across multiple active sources.

## Deliverables

- User `lock.toml` schema with:
  - schema version
  - source commit identities
  - active skills
  - unlinked skills with reasons
  - pending approvals
  - target materialization summary
- Lock write after successful resolution/materialization.
- Lock read for status drift comparison.
- Multi-source resolution in real `sync`.
- Stable serialization ordering.
- Lock compatibility behavior:
  - block on unsupported major schema
  - tolerate safe unknown minor fields

## Out of Scope

- Source lockfiles for pinned external/catalog sources.
- Lockfile PR generation.
- Catalog inventory snapshots.

## Acceptance Criteria

- Multi-source resolution is deterministic.
- Lockfile output is stable across identical inputs.
- Shadowed skills are recorded as `unlinked` with reason `shadowed`.
- Removing a source leaves owned symlinks eligible for orphan cleanup.
- Unsupported lock schema versions fail with an actionable error.
- `status` can explain current state using both live resolution and previous lock data.

## Validation

```sh
cargo fmt --check
cargo test lockfile resolver status
cargo test sync
git diff --check
```

## Suggested Issue Split

- Define user lock schema.
- Write lock after sync.
- Read lock for status.
- Add multi-source sync tests.
- Add schema-version tests.

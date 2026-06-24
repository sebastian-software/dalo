# M06: Team Sources and Git Safety

Status: todo  
Target: V1  
Depends on: M05, RFC 0002, RFC 0003  

## Goal

Add Git-backed team sources to the sync loop while preserving the no-data-loss model.

## Deliverables

- `skillmgr source add <id> <url>` for `team` sources.
- `skillmgr source list`.
- `skillmgr source priority`.
- Git wrapper for:
  - clone
  - fetch
  - explicit tracking update
  - `rev-parse HEAD`
  - `status --porcelain=v2`
- Dirty-check integration before source refresh.
- Clean tracking team source refresh during `sync`.
- Dirty source blocking for non-interactive sync.
- Store lock for mutating commands.
- Clear Git error messages with command context and stderr.

## Out of Scope

- External/catalog pinned sources.
- `source refresh`.
- PR creation.
- Forge adapters beyond raw Git remotes.

## Acceptance Criteria

- Adding a team source clones into the store, not into an agent target.
- `sync` refreshes clean tracking team sources before inventory scanning.
- Dirty team source state blocks refresh without discarding local edits.
- Failed Git commands produce actionable errors.
- Read-only commands do not take the store lock.
- Interactive mutating lock contention fails after the configured retry window.
- Scheduled/non-interactive lock behavior can be represented even before scheduler installation exists.

## Validation

```sh
cargo fmt --check
cargo test git source
cargo test sync
git diff --check
```

## Suggested Issue Split

- Implement Git command wrapper.
- Implement team source add/list/priority.
- Implement dirty checks.
- Integrate clean source refresh into sync.
- Implement store lock.

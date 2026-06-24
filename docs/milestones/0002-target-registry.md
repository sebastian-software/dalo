# M02: Target Registry

Status: todo  
Target: V1  
Depends on: M01, RFC 0001  

## Goal

Implement target detection and linking for the first directory-based agents without materializing skills yet.

## Deliverables

- Target registry entries for:
  - Codex: `~/.agents/skills`
  - Claude Code: `~/.claude/skills`
  - OpenClaw: `~/.agents/skills`
  - Hermes: `~/.hermes/skills`
  - generic folder target
- Experimental registry placeholders for Cursor and OpenCode marked as unverified.
- `skillmgr target detect`.
- `skillmgr target link <target>`.
- `skillmgr target unlink <target>`.
- Canonical path de-duplication for logical targets that share a physical directory.
- State updates that record logical targets and materialization directories separately.
- `--dry-run` and `--json` support.

## Out of Scope

- Skill symlink creation.
- Agent-specific instruction files.
- Cursor/OpenCode full support.
- Cloud-sync path warning unless cheap to add here; otherwise leave for `doctor`.

## Acceptance Criteria

- Detect reports known targets and whether their directories exist.
- Link creates missing target directories only after explicit command execution.
- Link never moves or deletes existing target contents.
- Unlink removes target configuration but does not remove target directories or unmanaged files.
- Codex and OpenClaw can both be configured while mapping to one canonical materialization directory.
- JSON output distinguishes logical target IDs from physical paths.

## Validation

```sh
cargo fmt --check
cargo test target
cargo run -- --store /tmp/skillmgr-test target detect --json
cargo run -- --store /tmp/skillmgr-test target link codex --dry-run
git diff --check
```

## Suggested Issue Split

- Add target registry data model.
- Implement detect.
- Implement link/unlink state changes.
- Add canonical path de-duplication tests.

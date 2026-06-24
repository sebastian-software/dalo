# M02: Target Registry

Status: done
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
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/skillmgr-test target detect --json
cargo run -- --store /tmp/skillmgr-test target link codex --dry-run
git diff --check
```

## Completion Notes

- Added the V1 target registry for Codex, Claude Code, OpenClaw, Hermes, and generic folder targets.
- Added experimental unverified registry entries for Cursor and OpenCode.
- Implemented `target detect`, `target link`, and `target unlink` with text and JSON output.
- Added optional path overrides for supported targets and required explicit paths for `generic`.
- Added canonical path de-duplication so logical targets sharing one physical directory produce one materialization directory.
- Ensured unlink removes configuration only and leaves target directories untouched.
- Added unit and command-level tests for registry contents, generic path requirements, link/unlink behavior, and physical-directory de-duplication.
- Validation passed on 2026-06-24.

## Suggested Issue Split

- Add target registry data model.
- Implement detect.
- Implement link/unlink state changes.
- Add canonical path de-duplication tests.

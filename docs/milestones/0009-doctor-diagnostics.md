# M09: Doctor and Diagnostics

Status: done
Target: V1  
Depends on: M08, RFC 0001, RFC 0002  

## Goal

Make dalo self-explanatory when something is wrong. `doctor` should inspect environment, store, Git, targets, locks, and owned symlinks without mutating state.

## Deliverables

- `dalo doctor`.
- Checks for:
  - store exists and has expected layout
  - config parses
  - state parses
  - lock parses
  - local source Git repository exists
  - `git` is available
  - `gh` is available and authenticated for PR flows
  - configured targets exist or can be created
  - duplicate canonical target directories are understood
  - owned symlinks point inside the store
  - broken owned symlinks
  - unmanaged same-name blockers
  - dirty sources
  - unapproved newly active skills
  - likely cloud-synced target paths when detectable
- Text and JSON diagnostic output.

## Out of Scope

- Automatic repair beyond pointing to explicit commands.
- Network checks.
- Scheduler checks, unless scheduler work is pulled forward.

## Acceptance Criteria

- `doctor` is read-only.
- Each finding has severity, code, message, and suggested next command.
- Broken owned symlinks are reported.
- Foreign symlinks are reported without being modified.
- Missing `gh` is only an error for PR-flow readiness, not for normal sync readiness.
- JSON output can be consumed by automation.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test --json doctor
git diff --check
```

Validated on 2026-06-24.

## Completion Notes

- Added `dalo doctor` with text and JSON output.
- Doctor is read-only and returns a report instead of mutating or repairing state.
- Each finding includes severity, code, message, and an optional next command.
- Added store layout, config, state, lock, local Git, `git`, `gh`, target, duplicate target directory, source dirtiness, pending approval, and cloud-synced path checks.
- Added owned symlink checks for valid, missing, broken, real-entry, and foreign symlink states.
- Added unmanaged same-name blocker reporting.
- Missing or unauthenticated `gh` is reported as PR-flow readiness, not normal sync readiness.
- Added unit and CLI tests for missing stores, initialized stores, broken symlinks, foreign symlinks, and unmanaged blockers.

## Suggested Issue Split

- Implement diagnostic model.
- Add store/config/source checks.
- Add target/symlink checks.
- Add Git/gh checks.
- Add doctor JSON snapshots.

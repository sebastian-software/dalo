# M08: Adopt and Minimal Resolve

Status: done
Target: V1  
Depends on: M07, RFC 0001, RFC 0003  

## Goal

Support the local development loop: detect unmanaged skills, copy them into the local source first, and provide small explicit repair commands for common blockers.

## Deliverables

- Unmanaged skill detection in configured targets.
- `dalo adopt <slot-or-path>`.
- Copy-first adoption into `~/.dalo/local/skills/<slot>`.
- Optional replacement of original folder with an owned symlink after confirmation.
- Local override status when adopted skill has the same slot name as a team skill.
- Minimal `resolve` commands:
  - `resolve list`
  - `resolve adopt <id>`
  - `resolve keep <id>`
  - `resolve remove-owned <id>`
- `--yes` behavior that confirms safe prompts but never creates commits.

## Out of Scope

- Full interactive resolve assistant.
- Automatic rename/adapt.
- Promote PR workflow.
- Dirty-team-edit promotion.

## Acceptance Criteria

- Adoption copies before replacing anything.
- `.local`-style or explicitly protected skills are not overwritten.
- Existing unmanaged folders are preserved unless the user confirms replacement.
- Adopted skills are immediately visible as local source skills.
- `--yes` does not commit local source changes.
- `resolve remove-owned` removes only recorded dalo-owned symlinks.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo test materialize
git diff --check
```

Validated on 2026-06-24.

## Completion Notes

- Added unmanaged skill discovery for linked target directories.
- `status` reports unmanaged target skills and whether they are protected.
- Added `dalo adopt <slot-or-path>` with copy-first adoption into the local source.
- Adoption only replaces the original target folder when `--replace` is passed.
- `.local` marker skills and explicitly kept skills are protected from replacement.
- `--replace` records an owned symlink but does not create Git commits.
- Adopted local skills participate in resolution immediately and show as local overrides over team skills.
- Added `resolve list`, `resolve adopt`, `resolve keep`, and `resolve remove-owned`.
- `resolve remove-owned` removes only recorded dalo-owned symlinks and blocks on real entries.

## Suggested Issue Split

- Implement unmanaged skill discovery.
- Implement copy-first adopt.
- Implement optional symlink replacement.
- Implement minimal resolve list/keep/remove-owned.
- Add local override tests.

# M14: Instruction Pack Managed Blocks

Status: done
Target: V1.1  
Depends on: M01, M05, RFC 0001, RFC 0002  

## Goal

Render versioned instruction packs into managed blocks inside agent instruction files (such as `AGENTS.md` or `CLAUDE.md`) without disturbing the surrounding, user-owned content. An instruction pack is a versioned Markdown artifact of standing agent-facing conventions, explicitly enabled, not auto-active.

## Deliverables

- Instruction pack model: a versioned Markdown artifact provided by a source. The store already reserves `local/instructions/` for user packs.
- Explicit enable: packs become active only through the user or team manifest, never implicitly.
- Managed block rendering into configured instruction-file targets using paired markers:
  `<!-- dalo:start <pack-id> -->` … `<!-- dalo:end <pack-id> -->`.
- Safe update: only the content between a pack's markers is (re)written; everything outside any managed block is preserved byte-for-byte.
- Disable/removal: disabling a pack removes its managed block and leaves the rest of the file untouched.
- The user lock records active instruction packs.

## Out of Scope

- Instruction pack discovery across sources (M15).
- Topic-overlap warnings (M15).
- Native include/import directives (deliberately not the baseline).
- Managing arbitrary dotfiles, editor settings, or unmarked instruction content.

## Acceptance Criteria

- Rendering only changes the bytes inside the pack's `dalo:start`/`dalo:end` markers.
- Content outside any managed block is byte-identical before and after render.
- A target file without an existing block gets the managed block added per the target rule; a second render is idempotent.
- Disabling a pack removes exactly its block.
- The user lock lists the active packs and their source commit.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test sync
git diff --check
```

## Suggested Issue Split

- Add the instruction pack model and manifest enable flag.
- Implement managed-block marker parsing and idempotent rendering.
- Implement safe block update and removal preserving outside content.
- Record active instruction packs in the user lock.
- Add unit and CLI tests for render/update/disable idempotence.

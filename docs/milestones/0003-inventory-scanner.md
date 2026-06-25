# M03: Inventory Scanner

Status: done
Target: V1  
Depends on: M01, RFC 0001, RFC 0003  

## Goal

Scan skills from local and team source checkouts into deterministic inventories. The scanner should understand enough `SKILL.md` metadata for V1 resolution without trying to normalize every possible external format.

## Deliverables

- Recursive scan for directories containing `SKILL.md`.
- Frontmatter parsing for V1 fields:
  - `id`
  - `name`
  - `description`
  - `requires`
  - `owners`
  - `tags`
- Slot-name derivation:
  - frontmatter `name` when present and valid
  - folder name fallback
- Stable source-qualified refs: `<source-id>:<slot-name>`.
- Deterministic ordering.
- Warnings for malformed frontmatter, invalid names, duplicate slot names within the same source, and unreadable skill directories.

## Out of Scope

- Catalog full inventory mode.
- Required-closure expansion.
- Instruction pack scanning.
- Semantic validation of skill content.

## Acceptance Criteria

- Identical source contents produce identical serialized inventory output.
- Folder-name fallback works for existing skills without frontmatter IDs.
- Unknown frontmatter fields are tolerated.
- Invalid skill paths or broken reads produce warnings, not panics.
- Duplicate slot names within a source are reported visibly and deterministically.

## Validation

```sh
cargo fmt --check
cargo test inventory
cargo test
cargo clippy --all-targets --all-features -- -D warnings
git diff --check
```

## Completion Notes

- Added recursive source scanning for directories containing `SKILL.md`.
- Added YAML frontmatter parsing for `id`, `name`, `description`, `requires`, `owners`, and `tags`.
- Added slot-name derivation from valid frontmatter `name` with folder-name fallback.
- Added stable source-qualified refs in the form `<source-id>:<slot-name>`.
- Added warnings for malformed frontmatter, invalid slot names, unreadable paths, and duplicate slot names within a source.
- Added unit tests for frontmatter parsing, fallback behavior, and duplicate detection.
- Validation passed on 2026-06-24.

> Note: an early version added deterministic SHA-256 content hashes over skill
> directories and metadata hashes over parsed frontmatter. These were removed in
> a later review together with the `sha2` dependency; content/metadata
> fingerprints return with V1.1 drift detection (see `src/inventory.rs`).

## Suggested Issue Split

- Implement `SKILL.md` discovery.
- Implement frontmatter parser and V1 metadata model.
- Implement stable hashing and ordering.
- Add malformed and duplicate inventory tests.

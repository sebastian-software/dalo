# M03: Inventory Scanner

Status: todo  
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
- Content and metadata hashes for inventory records.
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
git diff --check
```

## Suggested Issue Split

- Implement `SKILL.md` discovery.
- Implement frontmatter parser and V1 metadata model.
- Implement stable hashing and ordering.
- Add malformed and duplicate inventory tests.

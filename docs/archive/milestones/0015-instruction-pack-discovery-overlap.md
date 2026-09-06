# M15: Instruction Pack Discovery and Topic-Overlap Warnings

Status: done
Target: V1.1  
Depends on: M14, RFC 0001, RFC 0002  

## Goal

Make instruction packs discoverable across configured sources and warn when multiple active packs cover overlapping topics, so users notice conflicting standing conventions before they accumulate in agent instruction files.

## Deliverables

- Discover instruction packs in sources (analogous to skill inventory scanning): pack ID, source, version/commit, declared topics/tags.
- `status` lists available and enabled packs, with their source and active state.
- Topic-overlap detection: when two or more active packs declare overlapping topics/tags, emit a warning that names the involved packs.
- Overlap findings exposed in `status` (text + JSON) and `doctor`.

## Out of Scope

- Automatic conflict resolution or merging of overlapping packs.
- Semantic content analysis beyond declared topics/tags.
- Rename/adapt flows.

## Acceptance Criteria

- Discovery is read-only and never materializes a pack.
- `status` distinguishes available vs. enabled packs.
- Two active packs with overlapping declared topics produce a warning naming both pack refs.
- Overlap warnings never block materialization; they are advisory.
- Overlap findings appear in `status --json` and `doctor`.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test --json status
git diff --check
```

## Suggested Issue Split

- Implement instruction pack discovery/inventory.
- List available/enabled packs in `status`.
- Implement declared-topic overlap detection.
- Surface overlap warnings in `status` and `doctor`.

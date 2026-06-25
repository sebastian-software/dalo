# M11: Catalog Source and Selection

Status: todo
Target: V1.1  
Depends on: M03, M04, M07, RFC 0001, RFC 0003  

## Goal

Treat a multi-skill Git repository as an offer surface rather than an all-or-nothing dependency. Let a source inspect such a catalog, select only the skills it wants to expose, and pin the catalog commit plus the selection in a source lockfile. Only selected catalog skills enter the resolved skill set.

## Deliverables

- `catalog` source kind in the source schema (alongside `local` and `team`).
- `dalo source add-catalog <id> <url>`: register a catalog source and cache/check out the full repository.
- `dalo source inspect <id>`: read-only listing of candidate skills (path, stable ID when present, slot name, description, content/metadata fingerprints, declared `requires`). Does not materialize anything.
- `dalo source select <id> <skill...>` and a matching unselect: record the selected skills by stable ID (preferred) or path/name fallback.
- `source-lock.toml` (source lockfile) recording the catalog commit and the selected skill IDs/paths plus the inventory snapshot used to resolve them, with `schema_version`.
- Resolver/materializer integration: only selected catalog skills become candidates; unselected skills stay visible in `status` as available but are never materialized.
- Selection prefers stable `SKILL.md` IDs; path/name-based selections are recorded with a stronger fragility warning.

## Out of Scope

- Drift/update reporting between inventories (M12).
- Same-catalog `requires` closure expansion (M13).
- `source refresh` lockfile PRs (later slice).
- Catalog selection editing UX beyond add/select/unselect.

## Acceptance Criteria

- `source inspect` is read-only and never changes the resolved set.
- Selecting a skill that is not in the catalog inventory fails with a clear error.
- Unselected catalog skills appear in `status` as available-but-inactive, not as managed.
- The source lockfile records the catalog commit, the selection, and the inventory snapshot.
- Path/name-based selections surface a fragility warning; stable-ID selections do not.
- A newly active selected skill still requires local approval like any other team skill.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test source inspect <catalog-id>
git diff --check
```

## Suggested Issue Split

- Add the `catalog` source kind and `source add-catalog`.
- Implement `source inspect` over a cached catalog checkout.
- Implement `source select`/unselect and the selection store.
- Add the source lockfile (commit + selection + inventory snapshot).
- Wire selected-only catalog skills into the resolver and `status`.

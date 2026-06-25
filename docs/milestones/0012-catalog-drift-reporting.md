# M12: Catalog Drift Reporting

Status: todo
Target: V1.1  
Depends on: M11, RFC 0001, RFC 0003  

## Goal

Detect upstream catalog structure drift before it changes the resolved skill set. A read-only check fetches the catalog's upstream tracking ref and compares the resulting inventory against the pinned snapshot, reporting the difference as explicit, reviewable outcomes instead of silently adding or losing skills. The check never advances the pin or changes the resolved set.

## Deliverables

- Reintroduce content and metadata fingerprints for catalog inventory entries (the V1 cleanup removed the unused `content_hash`/`metadata_hash`; drift detection is their real consumer). Persist them in the source lockfile inventory snapshot.
- A read-only `dalo source refresh --check <id>` that fetches the catalog's upstream tracking ref and compares the fresh inventory against the pinned snapshot, without advancing the pin, writing a new lock, or changing the resolved set.
- Inventory comparison between the pinned snapshot and the freshly fetched inventory.
- The four update outcomes, each with a machine code and message:
  - `new_available`: upstream added a skill that is not selected (informational).
  - `selected_changed`: a selected skill changed content or metadata (reviewable).
  - `selected_moved`: a selected skill appears to have moved (auto-reconcilable only when the stable ID is unchanged).
  - `selected_removed`: a selected skill no longer exists (blocks scheduled sync for that selection).
- Surface drift in `status` (text + JSON) and as `doctor` findings.
- Move detection driven by stable ID; without a stable ID, path/name/content heuristics may warn about a likely move but must not rewrite the selection.

## Out of Scope

- Lock-advancing `source refresh` (writing new pins / lockfile PRs): only the read-only `--check` path is in scope here; advancing the pin is a later source-maintenance slice.
- Automatic reconciliation beyond stable-ID moves.
- Interactive drift resolution UX.

## Acceptance Criteria

- `source refresh --check` is read-only: it does not advance the pin, write a lock, or change the resolved set.
- Each outcome has a stable code and a human-readable message.
- `new_available` never changes the resolved set on its own.
- `selected_removed` blocks non-interactive sync for that selection until the manifest is updated.
- `selected_moved` is auto-reconciled only on a stable-ID match; otherwise it only warns.
- Drift is visible in both `status --json` and `doctor`.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test --json status
git diff --check
```

## Suggested Issue Split

- Reintroduce and persist content/metadata fingerprints for catalog entries.
- Implement locked-vs-current inventory comparison.
- Classify the four drift outcomes with codes.
- Surface drift in `status` and `doctor`.
- Add stable-ID move detection with path/name fallback warnings.

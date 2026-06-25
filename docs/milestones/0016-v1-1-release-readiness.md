# M16: V1.1 Release Readiness

Status: done
Target: V1.1  
Depends on: M11, M12, M13, M14, M15, RFC 0001, RFC 0002, RFC 0003  

## Goal

Harden the V1.1 surface the way M10 hardened V1: polish the new catalog and instruction-pack commands, align documentation with the implemented behavior, ensure test coverage and release gates, and make sure no field added in preparation for V1.1 is left dead.

## Deliverables

- CLI UX polish for the catalog (`source add-catalog`/`inspect`/`select`) and instruction-pack commands, including text and JSON output consistency.
- Documentation alignment: README V1.1 sections, RFC cross-references, and milestone status updates reflect the shipped surface.
- Test coverage: catalog select/drift/closure flows and instruction-pack render/discovery/overlap, including JSON schema snapshots; no network access in tests.
- Confirm the V1.1-prep fields are now genuinely consumed: `requires` (M13), content/metadata fingerprints (M12), `tags` (instruction-pack topics / catalog discovery). Remove anything still unused.
- Release gates green and a clean release-please changelog entry for the V1.1 version.

## Out of Scope

- Post-V1.1 work: scheduled autosync installer, `source refresh` lockfile PRs, full PR-first `promote`, rename/adapt flows, additional verified agent adapters, forge adapters beyond GitHub.

## Acceptance Criteria

- Catalog and instruction-pack flows have command tests covering happy paths and the key safety guarantees (selection, drift blocking, closure blocking, managed-block isolation).
- `status --json` and `doctor` JSON for the new surfaces are snapshot-tested.
- Documentation matches the implemented V1.1 behavior; no doc references a removed or unimplemented field.
- No dead prepared field remains: every field is read by a consumer or removed.
- `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets --all-features -- -D warnings`, and the release build are green.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
git diff --check
```

## Suggested Issue Split

- Polish catalog command UX and output.
- Polish instruction-pack command UX and output.
- Add JSON schema snapshot tests for the new surfaces.
- Audit and reconnect or remove V1.1-prep fields.
- Update README, RFCs, and milestone statuses for V1.1.

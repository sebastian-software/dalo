# M10: V1 Release Readiness

Status: done
Target: V1  
Depends on: M09, README, RFC 0001, RFC 0002, RFC 0003  

## Goal

Turn the implemented V1 loop into a coherent first release candidate: documented, tested, packaged, and honest about deferred scope.

## Deliverables

- CLI help pass across all implemented commands.
- README updated from product narrative to include real installation and quickstart steps.
- RFC status review:
  - mark implemented sections
  - record deferred sections
  - capture any intentional deviations
- End-to-end fixtures:
  - local-only sync
  - team source sync from local bare Git repo
  - unmanaged conflict
  - adoption flow
  - multi-source shadowing
  - dirty source block
- Release build validation for macOS and Linux where available.
- Basic GitHub Actions workflow if repository policy allows it.
- Versioning decision for first tag.

## Out of Scope

- Homepage.
- Homebrew tap.
- Cargo publish.
- V1.1 catalog and instruction-pack implementation.

## Acceptance Criteria

- A new user can follow README quickstart against a temporary store.
- All V1 commands either work or are absent; no misleading stub commands remain in primary help.
- Test suite does not require network access.
- Release notes clearly state what is V1 and what is deferred.
- Safety guarantees in README are backed by tests.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
git diff --check
```

Validated on 2026-06-24.

## Completion Notes

- Added command help coverage for all implemented V1 command groups.
- Updated README with source-build installation, a temporary-store quickstart, local team-source quickstart, real V1 status, and deferred scope.
- Added V1 implementation status review for RFC 0001, RFC 0002, and RFC 0003.
- Added draft `v0.1.0-rc.1` release notes and versioning decision.
- Added end-to-end fixtures for local-only sync, local Git team source sync, unmanaged conflict, adoption flow, multi-source shadowing, and dirty source blocking.
- Added Linux/macOS GitHub Actions CI for format, tests, Clippy, and release build.
- Adjusted `source add` so explicitly added team sources are locally approved for V1 resolution.
- Validated release build with `cargo build --release`.

## Suggested Issue Split

- Add end-to-end fixture tests.
- Update README quickstart from real commands.
- Review RFC implementation status.
- Add CI workflow.
- Cut first release candidate.

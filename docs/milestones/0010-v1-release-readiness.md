# M10: V1 Release Readiness

Status: todo  
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
cargo build --release
git diff --check
```

## Suggested Issue Split

- Add end-to-end fixture tests.
- Update README quickstart from real commands.
- Review RFC implementation status.
- Add CI workflow.
- Cut first release candidate.

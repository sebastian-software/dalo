## Summary

- 

## User-visible impact

- 

## Safety notes

- 

## Validation

CI runs all of these on Linux and macOS. Tick the ones you ran; say why for any
you skipped. `tests/workflows.sh` checks this list against the CI job it mirrors.

- [ ] `cargo fmt --check`
- [ ] `cargo test --locked`
- [ ] `sh tests/install.sh`
- [ ] `sh tests/docs.sh`
- [ ] `sh tests/workflows.sh`
- [ ] `(cd npm && npm ci && npm run check-version && npm test)`
- [ ] `cargo clippy --locked --all-targets --all-features -- -D warnings`
- [ ] `cargo build --release --locked`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features`
- [ ] `cargo deny check`
- [ ] `cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines "$(cat coverage-threshold)"`
- [ ] `git diff --check`

## Linked issues

Closes #

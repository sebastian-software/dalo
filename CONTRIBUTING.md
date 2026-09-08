# Contributing to Dalo

Thanks for improving Dalo. This project is a Rust CLI with a reusable library core, and many changes affect user files, Git checkouts, symlinks, or generated lock state. Keep changes small, tested, and explicit about the behavior they change.

## Start Here

Useful project references:

- User-facing command and file reference: [docs/reference.md](docs/reference.md)
- Implementation milestones and validation policy: [docs/archive/milestones/README.md](docs/archive/milestones/README.md)
- Product and architecture background: [docs/rfcs/](docs/rfcs/)
- Release history: [CHANGELOG.md](CHANGELOG.md)

## Development Setup

Install the Rust toolchain used by the project and Node.js 24, then run the
checks that CI runs on every supported OS:

```sh
cargo fmt --check
cargo test --locked
sh tests/install.sh
sh tests/docs.sh
sh tests/workflows.sh
(cd npm && npm ci && npm run check-version && npm test)
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --release --locked --target "$(rustc -vV | sed -n 's/^host: //p')"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

The MSRV, dependency-audit, coverage, and site-render jobs additionally run:

```sh
cargo check --locked --all-targets --all-features
cargo deny check
cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines "$(cat coverage-threshold)"
node site/build.mjs --check
```

The site check compares the committed render under `site/` with what
`site/build.mjs` produces from `docs/*.md`; it needs the renderer installed
once with `pnpm --dir site install`. `site/README.md` describes the build.

The MSRV job installs the toolchain named by `rust-version` in `Cargo.toml`, so
that number lives in one place. To run the same check locally:

```sh
msrv="$(sed -n 's/^rust-version = \"\(.*\)\"/\1/p' Cargo.toml | head -n 1)"
cargo +"$msrv" check --locked --all-targets --all-features
```

The standards-drift job runs `standards check` from
[`@sebastian-software/standards`](https://github.com/sebastian-software/standards)
at the version pinned in the job itself. This repository has no lockfile to
hold that CLI, so the pin lives in `.github/workflows/ci.yml` and Renovate's
custom manager in `renovate.json` moves it through a pull request instead of CI
running whatever was published last. Copy the command from the job rather than
restating the version here, and raise the pin in the same pull request that
runs `standards apply`, so the stamp in `.repometa.json` and the CLI that
checks it never disagree.

`coverage-threshold` holds the line-coverage gate, and both CI and the command
above read it, so the number is never restated. Raise it deliberately when
sustained coverage improvements establish a new baseline; do not lower it to
make an untested change pass.

This list, the checklist in `.github/pull_request_template.md`, and the CI jobs
are kept in step by `tests/workflows.sh`; update all three together.

Use `git diff --check` before opening a PR to catch whitespace issues.

## Testing Expectations

Run the narrowest useful validation while developing, then run the relevant full checks before opening a PR.

For a fast command-level loop, run a focused integration test such as:

```sh
cargo test --test cli -- source_refresh
```

- Code formatting changes should pass `cargo fmt --check`.
- Behavior changes should pass `cargo test`.
- CLI behavior changes should include command-level tests in `tests/cli.rs` when practical.
- End-to-end workflow changes should include or update tests in `tests/e2e.rs`.
- Tests must not require network access.
- Source, store, lock, approval, symlink, and instruction-pack changes should include failure-path coverage where the behavior protects user data.

If a command can mutate user files, prefer `--dry-run` coverage and tests for the blocked/unsafe path as well as the success path.

## Commit Messages

Dalo uses release-please, so commit messages should follow Conventional Commits. Release notes and version bumps are inferred from commits on `main`.

Common prefixes:

```text
feat: add a user-visible feature
fix: repair a bug or unsafe behavior
docs: update documentation only
test: add or adjust tests
ci: update GitHub Actions or release automation
chore: maintenance without user-visible behavior
refactor: restructure code without changing behavior
```

Use a scope only when it adds clarity:

```text
fix(sync): preserve owned records when a source scan is degraded
docs(reference): document JSON output shapes
```

Breaking changes should use the Conventional Commits breaking-change syntax:

```text
feat!: change the config schema

BREAKING CHANGE: config schema version 1 is no longer accepted.
```

## Pull Requests

A good PR includes:

- A focused summary of user-visible behavior and safety implications.
- Linked issue text such as `Closes #123` when applicable.
- Validation commands that were actually run.
- Screenshots or command output only when they clarify a user-facing behavior.
- Updates to `README.md`, [docs/reference.md](docs/reference.md), milestones, or release notes when behavior changes what users should know.

Keep PRs focused. If a fix uncovers unrelated cleanup, open a follow-up issue or separate PR unless the cleanup is required to make the change safe.

## Safety Guidelines

Dalo manages Git checkouts and symlinks content into agent folders. Be conservative:

- Do not overwrite unmanaged real directories or foreign symlinks.
- Preserve user-authored files outside Dalo-owned managed blocks.
- Keep dirty source behavior explicit and blocking.
- Keep persisted schema changes versioned and documented.
- Treat local paths, Git URLs, and source IDs as untrusted input.
- Prefer atomic writes for store files and target instruction files.

When in doubt, make the operation report a blocked state instead of mutating ambiguous user content.

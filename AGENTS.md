# AGENTS.md

Guidance for coding agents working in this repository. Humans welcome too.

## What this is

Dalo is an MIT-licensed Rust CLI with a reusable library core. It manages Git
checkouts and symlinks skill content into agent folders, so most changes touch
user files, lock state, or symlinks.

## Language

English is the project language
([docs/adr/0001-project-language.md](docs/adr/0001-project-language.md)): code,
comments, CLI copy, documentation, commit messages, issues, and pull requests.

## Commits

Conventional Commits without exception — release-please derives release notes
and version bumps from the commits on `main`. See
[CONTRIBUTING.md](CONTRIBUTING.md#commit-messages) for the prefixes in use.

## Preflight

[CONTRIBUTING.md](CONTRIBUTING.md#development-setup) is canonical. These are the
checks CI runs on every supported OS:

```sh
cargo fmt --check
cargo test --locked
sh tests/install.sh
sh tests/docs.sh
sh tests/workflows.sh
(cd npm && npm ci && npm test)
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
```

Run the narrowest useful check while developing, then the relevant full checks
before opening a pull request. `tests/docs.sh` asserts documentation invariants
across `README.md`, `site/`, and `docs/`, and it builds the debug binary to
drive real CLI flows — run it after any documentation or user-facing behavior
change.

## Safety

Dalo mutates user files, so follow
[CONTRIBUTING.md](CONTRIBUTING.md#safety-guidelines): never overwrite unmanaged
directories or foreign symlinks, preserve user-authored content outside
Dalo-owned managed blocks, keep dirty-source behavior explicit and blocking,
keep persisted schema changes versioned and documented, and treat local paths,
Git URLs, and source IDs as untrusted input. When in doubt, make the operation
report a blocked state instead of mutating ambiguous user content.

## Where decisions live

- [docs/adr/](docs/adr/) — architecture decision records
- [docs/rfcs/](docs/rfcs/) — product and architecture background
- [docs/milestones/README.md](docs/milestones/README.md) — implementation
  milestones and validation policy
- [docs/reference.md](docs/reference.md) — user-facing command and file
  reference

---

<!-- sebastian-software-consumer-agents:start -->

# Standards-managed repo guardrails

- Do not hand-edit managed files or standards-owned marker sections.
- If `standards check` reports drift, run `standards apply` or update standards.
- `pnpm agent:check` may omit `standards check`; CI can still fail on drift.
- Fix or format every file reported by `oxfmt` whenever practical.
- For generated files, prefer formatting in the generator step.
- If formatting is not viable, use repo-local `.prettierignore`.
- Never add repo-specific ignores to managed `.oxfmtrc.json`.
<!-- sebastian-software-consumer-agents:end -->

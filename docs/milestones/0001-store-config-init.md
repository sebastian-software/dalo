# M01: Store, Config, and Init

Status: done
Target: V1  
Depends on: M00, RFC 0001, RFC 0002  

## Goal

Implement the local store and `dalo init` so the tool can create a safe, inspectable foundation under `~/.dalo` or an explicit `--store` path.

## Deliverables

- Store path resolution:
  - `--store <path>`
  - environment override if adopted during implementation
  - default `~/.dalo`
- Store layout creation:
  - `config.toml`
  - `lock.toml`
  - `state.toml`
  - `approvals.toml`
  - `local/`
  - `sources/`
- Local source initialization as a Git repository under `local/`.
- Typed TOML schemas for config, state, approvals, and user lock.
- Atomic write helper for generated TOML files.
- `dalo init` with `--dry-run`, `--json`, and idempotent behavior.
- Existing store detection with clear output.

## Out of Scope

- Target linking.
- Team source clone/fetch.
- Inventory scanning.
- Scheduler installation.

## Acceptance Criteria

- Running `init` twice does not corrupt or rewrite unrelated state.
- `init --dry-run` reports planned operations and writes nothing.
- Existing files outside dalo-owned store paths are not touched.
- The local source is a valid Git repository after `init`.
- Invalid store paths produce actionable errors.
- JSON output has stable fields for created, existing, and skipped operations.

## Validation

```sh
cargo fmt --check
cargo test store config lockfile
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test init --dry-run
cargo run -- --store /tmp/dalo-test init
cargo run -- --store /tmp/dalo-test init
git diff --check
```

## Completion Notes

- Added store path resolution from `--store`, `DALO_STORE`, or `~/.dalo`.
- Added typed TOML schemas for user config, user lock, state, and approvals.
- Added atomic TOML writes for generated store files.
- Implemented `dalo init` with text output, JSON output, dry-run planning, idempotent behavior, and local Git repository initialization.
- Added unit and command-level tests for dry-run, real init, idempotence, and layout creation.
- Validation passed on 2026-06-24.

## Suggested Issue Split

- Define persisted TOML schemas.
- Implement store path resolution and layout.
- Implement atomic TOML writes.
- Implement local Git source initialization.
- Add `init` text and JSON output tests.

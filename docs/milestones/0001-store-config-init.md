# M01: Store, Config, and Init

Status: todo  
Target: V1  
Depends on: M00, RFC 0001, RFC 0002  

## Goal

Implement the local store and `skillmgr init` so the tool can create a safe, inspectable foundation under `~/.skillmgr` or an explicit `--store` path.

## Deliverables

- Store path resolution:
  - `--store <path>`
  - environment override if adopted during implementation
  - default `~/.skillmgr`
- Store layout creation:
  - `config.toml`
  - `lock.toml`
  - `state.toml`
  - `approvals.toml`
  - `local/`
  - `sources/`
  - `logs/`
- Local source initialization as a Git repository under `local/`.
- Typed TOML schemas for config, state, approvals, and user lock.
- Atomic write helper for generated TOML files.
- `skillmgr init` with `--dry-run`, `--json`, and idempotent behavior.
- Existing store detection with clear output.

## Out of Scope

- Target linking.
- Team source clone/fetch.
- Inventory scanning.
- Scheduler installation.

## Acceptance Criteria

- Running `init` twice does not corrupt or rewrite unrelated state.
- `init --dry-run` reports planned operations and writes nothing.
- Existing files outside skillmgr-owned store paths are not touched.
- The local source is a valid Git repository after `init`.
- Invalid store paths produce actionable errors.
- JSON output has stable fields for created, existing, and skipped operations.

## Validation

```sh
cargo fmt --check
cargo test store config lockfile
cargo run -- --store /tmp/skillmgr-test init --dry-run
cargo run -- --store /tmp/skillmgr-test init
cargo run -- --store /tmp/skillmgr-test init
git diff --check
```

## Suggested Issue Split

- Define persisted TOML schemas.
- Implement store path resolution and layout.
- Implement atomic TOML writes.
- Implement local Git source initialization.
- Add `init` text and JSON output tests.

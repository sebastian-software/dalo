# M00: Repository and Toolchain Scaffold

Status: todo  
Target: V1  
Depends on: RFC 0002  

## Goal

Create the Rust project foundation for a single `skillmgr` binary backed by a reusable library. This milestone should establish the development loop without implementing product behavior beyond a minimal CLI shell.

## Deliverables

- `Cargo.toml` for one package with `skillmgr` binary and library target.
- `rust-toolchain.toml` pinned to stable.
- Rust 2024 edition.
- Initial module skeleton matching RFC 0002:
  - `cli`
  - `config`
  - `lockfile`
  - `store`
  - `source`
  - `inventory`
  - `resolver`
  - `materialize`
  - `target`
  - `git`
  - `status`
  - `doctor`
  - `error`
- Baseline dependencies:
  - `clap`
  - `serde`
  - `toml`
  - `serde_json`
  - `thiserror`
  - `anyhow`
  - `tempfile`
- Basic `skillmgr --help` and `skillmgr --version`.
- Test harness foundation for library tests and command-level tests.

## Out of Scope

- Store mutation.
- Real config parsing.
- Agent detection.
- Git commands.
- Symlink creation.

## Acceptance Criteria

- `cargo test` passes.
- `cargo fmt --check` passes.
- `skillmgr --help` lists the planned top-level command groups, even if most are not implemented yet.
- The binary exits with a clear "not implemented yet" error for stubbed commands.
- Core behavior is reachable through library modules rather than being embedded directly in `main.rs`.

## Validation

```sh
cargo fmt --check
cargo test
cargo run -- --help
git diff --check
```

## Suggested Issue Split

- Scaffold Cargo project and stable toolchain.
- Add CLI shell and global flags.
- Add initial module tree and error type.
- Add baseline CI command documentation.

# Store fixtures from released binaries

Each directory here is a Dalo store written by a **released** Dalo binary, not
by the current tree. `tests/upgrade.rs` copies one to a temporary root and runs
the current binary against it, which is the only way to prove that a store a
team has carried since 0.6 still opens without manual steps.

## How they were made

Every binary was installed from crates.io into a throwaway root, so nothing on
the machine was modified:

```sh
cargo install dalo --version 0.6.0 --root /tmp/dalo-0.6.0 --locked
```

All four versions built with `--locked` on the current toolchain; no release
archive fallback was needed. Each binary then built a store under a fixed,
synthetic root (`/private/tmp/dalo-fixture/<version>`) with:

- `init`, and a `generic` target linked to a directory in that root
- one local skill (`local/skills/local-note`)
- one team source added from a local Git repository with one skill
- one catalog source added from a local Git repository with two skills, one
  selected and approved
- one enabled local instruction pack rendered into an instruction file
- one protected unmanaged skill (`dalo resolve keep keep-mine`)
- `sync`

## What each fixture carries

| Version | `config` | `state` | `lock` | `source-lock` | Also exercises |
| --- | --- | --- | --- | --- | --- |
| 0.6.0 | 1 | 1 | 1 | 2 | path-only `protected_skills`, a hand-authored bare approval |
| 0.9.2 | 1 | 1 | 1 | 3 | — |
| 0.12.0 | 2 | 1 | 4 | 3 | — |
| 0.15.1 | 2 | 1 | 6 | 3 | control: every schema already current |

0.6.0 predates the `approve` command, so its approval records could only be
hand-authored. The fixture carries both shapes a 0.6.0 store could hold: a
source-qualified `public:copy-editing` (honoured then and now) and a bare
`launch-copy` (already rejected in 0.6.0, and still reported today).

## What the test must relocate

A released binary persists absolute paths, so a fixture cannot be used where it
lies. `fixture.toml` records everything that has to be rewritten:

- `origin_root` — the root the fixture was built under. Every occurrence in
  every UTF-8 file under the fixture is replaced with the temporary root. That
  one substitution covers the store path, the target path, the instruction file
  path, the source URLs in `config.toml`, and the `remote.origin.url` in each
  checkout's Git config.
- `git_dir_placeholder` — nested Git directories cannot be committed as `.git`,
  so each was renamed to `dot-git`. The test renames them back first.
- `[[repos]]` — each bare repository is copied to `<root>/<path>` so the source
  URL recorded in `config.toml` resolves. No network access is involved.
- `[[owned_links]]` — the owned symlinks the fixture's target directory held.
  They are recorded rather than committed, so the repository never carries a
  dangling symlink (which would also break `cargo package`). The test recreates
  each one and asserts that no upgrade removes it.

## What was stripped

Only content that is machine-specific or pure noise: the coarse `.lock` and
`.catalog.lock` files, `*.log`, and each Git directory's `hooks/` samples,
`logs/`, `description`, `FETCH_HEAD`, and `ORIG_HEAD`. Every persisted Dalo file
is byte-for-byte what the released binary wrote.

## Adding a fixture

Install the release into a throwaway root, build the same store shape, copy it
here with nested `.git` directories renamed to `dot-git`, and write a
`fixture.toml` describing it. `tests/upgrade.rs` discovers directories
automatically, so no test code changes are needed.

# Project installations

Dalo restores a project's skills from a checked-in definition into an independent
store. Inside a configured project, including its subdirectories, use:

```sh
dalo install
dalo status
dalo doctor --check
```

Dalo searches upward for the nearest `dalo-project.toml`, stopping at the nearest
Git repository boundary (including worktrees and submodules). Without a Git
boundary it searches up to the filesystem root. A malformed or redirected
definition fails instead of silently selecting the global store.

## Choosing scope

| Selection | Behavior |
| --- | --- |
| `--global`, `-g` | Use `~/.dalo`, bypassing project discovery and `DALO_STORE`. |
| `--project <directory>` | Use that exact project directory, without parent discovery. |
| `--store <path>` | Use an explicitly chosen store, bypassing project discovery. |
| `DALO_STORE` | Preserve an explicitly configured environment store, bypassing discovery. |
| No override, project definition found | Use the nearest project's `.dalo` store. |
| No override, no project definition | Preserve the global default, `~/.dalo`. |

The three CLI scope flags conflict with one another. Team authoring, completions,
manpages, and standalone plugin validation do not acquire an automatic project
scope. Unsupported project commands fail rather than operating globally.

To set up a project, run `dalo init` interactively inside its Git repository.
If no definition or store override exists, Dalo asks whether to initialize the
project (at the Git root) or the global store. Enter, EOF, or an unknown answer
cancels without writing. JSON, dry-run, CI, and non-interactive invocations never
prompt; without a definition they retain the existing global behavior. Choose
explicitly in scripts:

```sh
dalo --project . init  # Create this project's definition.
dalo init --global    # Initialize the home store.
```

Human project output identifies the scope. Existing JSON status and sync reports
retain their schemas and identify the project store through their store path.

## Definition and local state

Commit `dalo-project.toml`. It is a separate, versioned format from the
existing team-source `dalo.toml`; both can coexist in a repository.

```toml
schema_version = 1
targets = ["claude", "codex"]

[[source]]
id = "team"
url = "https://github.com/example/team-skills.git"
# Replace with the complete lowercase commit ID from this repository.
commit = "0123456789abcdef0123456789abcdef01234567"
skills = ["review", "documentation"]
```

Sources use explicit catalog skill selectors (name, stable ID, or relative
skill path). Dependencies use the existing resolver. Sources earlier in the
list take precedence over later sources. Supported targets are `claude`,
`codex`, `opencode`, `hermes`, and `openclaw`; Codex and OpenClaw share
`.agents/skills`, so choose only one of those target IDs.

`init` creates the definition only and never replaces an existing one. `install`
creates `.dalo/` inside the project, clones the declared sources at their exact
commits, and links approved skills into the selected agents' project folders.
Each fresh clone gets its own store and local approval decisions. The global
store and global agent folders are not part of this installation.

Add the following entries to your project's `.gitignore`, selecting only the
agent folders managed by this definition:

```gitignore
/.dalo/
/.claude/skills/
/.agents/skills/
```

Dalo does not edit `.gitignore` or take over existing skill directories. Keep
project-authored skills that are already versioned; a same-name unmanaged entry
blocks delivery instead of being replaced. Generated links are absolute paths
into the local store and must not be committed. The store's machine-specific
locks and approvals must not be committed either.

## Review and installation

The definition declares desired content, not approval. Initial installation
prepares the pinned sources but exits nonzero when approval is pending. Review
and approve skills locally, then rerun installation:

```sh
dalo status
dalo audit team:review
dalo approve skill team:review
dalo install
dalo doctor --check
```

Security findings still require the existing explicit, content-bound review
flow. Installation never auto-approves skills or executes their contents.
Repeated installation preserves the pins even if upstream has advanced.
Dirty checkouts, changed pins, redirected output directories, and unmanaged
same-name entries are not overwritten. After a failed initial preparation,
registered sources remain available for inspection and a retry; an
unregistered leftover checkout is reported for manual preservation/recovery.

`--dry-run install` validates and previews the declaration without fetching,
creating a store, auditing remote content, or delivering links. It is not a
promise that the remote sources or approvals are ready.

## First implementation boundary

This first increment supports `init`, `install`, `status`, `doctor`, `audit`,
and `approve` in project scope. Other project-scoped commands fail
instead of falling back to the global store. Use `install` to reconcile delivery;
`sync` is not yet supported in project scope. Use `dalo sync --global` when you
intend to synchronize the global store from inside a project.

Full commit IDs in the definition currently provide the reproducibility
boundary. Moving refs, a separate portable project lockfile, an update command,
and removal or replacement of previously installed sources/selections are
follow-up work. Changes to an existing pin, URL, or selection are reported and
left untouched. Do not discard local edits or approvals to work around that
boundary. A future update flow must preserve them while reviewing the change.

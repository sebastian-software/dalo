# Project installations

Dalo can restore a project's skills from a checked-in definition into an
independent store. Select the project explicitly on every command:

```sh
dalo --project . init
# Edit dalo-project.toml to declare sources and complete commit IDs.
dalo --project . install
```

Without `--project`, commands retain their existing store precedence:
`--store`, then `DALO_STORE`, then `~/.dalo`. Merely entering a repository does
not change scope. `--project <directory>` ignores `DALO_STORE` and conflicts
with `--store`. It names the exact project directory, without searching parents.
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
dalo --project . status
dalo --project . audit team:review
dalo --project . approve skill team:review
dalo --project . install
dalo --project . doctor --check
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
and `approve` with explicit project scope. Other project-scoped commands fail
instead of falling back to the global store. Use `install` to reconcile delivery;
plain `sync` keeps its existing global/custom-store meaning.

Full commit IDs in the definition currently provide the reproducibility
boundary. Moving refs, a separate portable project lockfile, an update command,
and removal or replacement of previously installed sources/selections are
follow-up work. Changes to an existing pin, URL, or selection are reported and
left untouched. Do not discard local edits or approvals to work around that
boundary. A future update flow must preserve them while reviewing the change.

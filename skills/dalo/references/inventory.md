# Inspect an existing setup

The assessment is read-only and works even before Dalo is installed. Use the
host's filesystem and shell tools; do not require another runtime or install
`npx skills` to learn what is on disk.

## Establish scope

Record the current project, requested agent(s), and relevant environment
overrides without dumping the environment or credentials. Resolve the store
from the user's explicit path, then `DALO_STORE`, then `~/.dalo`. Resolve relative
paths against the current working directory and retain that absolute store path
throughout the task. Do not silently switch stores if this one is broken.

If `dalo` is available, read its version and help. For supported commands use:

```sh
dalo --store "$store" --json next
dalo --store "$store" --json target detect
```

For an existing store also read `status`, `doctor`, `source list`, and
`resolve list` with the same `--store` and `--json` flags. Missing commands on an
older binary are compatibility information; use available reports and inspect
files read-only. Distinguish a missing store from malformed state, an unsupported
schema, missing permissions, or an unavailable source checkout.

## Inspect folders beyond Dalo's current targets

Dalo's unmanaged inventory covers configured target directories and excludes
foreign symlinks. Supplement it with a bounded filesystem scan, especially
before setup or migration.

Start with paths reported by `target detect` and paths the user named. For an
initial general assessment, inspect existing user-level `.agents/skills`,
`.codex/skills`, `.claude/skills`, `.hermes/skills`, `.config/opencode/skills`,
and `.cursor/skills` under the home directory, respecting discovered agent
configuration such as `CODEX_HOME` or `XDG_CONFIG_HOME`. In the current project
inspect existing `.agents/skills`, `.claude/skills`, `.opencode/skills`,
`.hermes/skills`, and `.cursor/skills`. Include ancestor skill folders only up
to the current repository root when the host discovers skills there. Do not
search the entire home directory, other repositories, or plugin caches by
default. A narrow maintenance request needs only its affected locations.

List skill entries using symlink-aware metadata (`lstat`/`readlink` or the host
equivalent). Record each visible path, its scope, whether it is a real directory
or symlink, its link destination, and whether a readable `SKILL.md` exists.
Resolve enough of a link chain to identify the shared content; detect cycles
and broken links instead of recursively traversing them. Group paths resolving
to the same content while retaining every agent-facing path. Do not count one
shared skill as several independent copies.

Use the following classifications, retaining uncertainty:

| Evidence | Classification |
| --- | --- |
| Dalo reports a recorded owned link and the on-disk link matches | Dalo managed |
| A recorded owned path is missing, repointed, or replaced by a real directory | Dalo ownership drift; investigate before adoption |
| Installer metadata matches the path and source, with content still to compare | External install, provenance candidate |
| Real skill directory without reliable provenance | Local or unknown; preserve as user content |
| Symlink without a matching Dalo record | Foreign link, even if its destination is in the store |
| Directory cannot be read, link is broken, or metadata cannot be parsed | Unresolved; not absent |

Names alone establish neither common origin nor equal contents. Do not label
an untracked skill “user-authored” as a fact without evidence. Before deduplication
or replacement compare complete trees, including resources, executable bits,
empty directories, and symlink destinations, rather than only `SKILL.md`.

## Recognize skills.sh / the skills CLI

skills.sh is a discovery directory; `npx skills` is its installation CLI. Their
presence does not imply that all files in `.agents/skills` belong to them.

Look for the nearest `skills-lock.json` from the working directory upward,
stopping at the nearest Git boundary (including worktrees and submodules), or
inspect the exact project the user selected. Record its project root so a lock
in an ancestor is not mistaken for a global install. Read that lock and the global lock at
`$XDG_STATE_HOME/skills/.skill-lock.json` when that override is set, otherwise
`~/.agents/.skill-lock.json`. If both global locations exist, report the fallback
as potentially stale rather than merging their authority. Preserve original
bytes; unknown schemas and malformed files remain unresolved evidence.

The project lock can include `source`, `sourceType`, `sourceUrl`, `ref`,
`skillPath`, and `computedHash`; the global lock can include `sourceUrl`, `ref`,
`skillPath`, and `skillFolderHash`. Record only relevant provenance and redact
credentials in URLs. A global folder hash is a Git tree identifier, not a
repository commit; it cannot be passed as a Dalo revision. A project content
hash uses a different algorithm from Dalo's. Neither proves the current local
tree is unmodified without an appropriate comparison.

These are discovery hints, not a persisted format Dalo owns. If the installed
format differs, consult the matching version of the upstream implementation:
[global lock](https://github.com/vercel-labs/skills/blob/main/src/skill-lock.ts),
[project lock](https://github.com/vercel-labs/skills/blob/main/src/local-lock.ts),
and [installation behavior](https://github.com/vercel-labs/skills#installation-methods).

Summarize the useful facts: existing Dalo health, agents and scope, shared versus
independent copies, candidate origins, and content that must be preserved.
Offer a recommendation based on those facts instead of asking the user which
technical state they are in. A project `skills-lock.json` is a trigger to read
[the migration suggestion guidance](migration.md#suggest-migration-from-the-inventory)
and propose a verified preview during a general assessment. Do not wait for the
user to discover or name the migration command.

## Project and global scope

Look for `dalo-project.toml` when a request concerns a repository. Dalo discovers
the nearest definition above the working directory, stopping at the nearest
Git boundary. In that scope use `dalo project add` to preview an explicitly
selected source and skill set, then apply the reviewed commit and run
`dalo install`. Use `dalo project update` to review a new commit and selected
inventory for an existing source before applying it and running `dalo install`.
Also use `status`, `doctor`, `audit`, and `approve` as needed.
Use `--global` for an explicitly global request and `--project <dir>` when
selecting a different project. Explicit `--store` and `DALO_STORE` bypass
automatic discovery. A malformed project definition must not cause a fallback
to global operations. Do not redirect global targets to implement a project.

`init` asks for project versus global scope only in an interactive, unconfigured
Git repository without overrides. Agents should ask in their own UI when the
intent is unclear, then use `--project <dir> init` or `init --global` explicitly.
JSON, dry-run, CI, and non-interactive calls never prompt.

The project workflow supports `project add`, `project update`, `project remove`, `init`,
`install`, `status`, `doctor`, `audit`, and `approve`. Add and update preview by
default; applying a branch or tag requires the exact `--expect-commit` from that
preview. Update preserves selection by stable ID unless `--skill` explicitly
replaces it. `.dalo` and generated links are machine-local; commit only
`dalo-project.toml`. Do not confuse it with the team-source `dalo.toml`. See
`https://dalo.sh/docs/projects.html` for the format and limitations. Preview
`dalo project remove` for explicit selector or source removal; apply the
declaration and run install to reconcile it. Required skills stay active through
remaining consumers, and removed sources' cached checkouts are retained even
when dirty. Source URL replacement remains separate migration work.
A global assistant bundle check needs `--global` (or an explicit custom store)
inside a project; keep that check separate from the requested project operation.

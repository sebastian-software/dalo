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

Read existing `skills-lock.json` in the project and the global lock at
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
technical state they are in.

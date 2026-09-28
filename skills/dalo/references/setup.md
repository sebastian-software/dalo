# Set up Dalo through the current agent

Complete the inventory first. An existing store should enter maintenance or
migration; do not recreate it as a shortcut.

## Binary missing or too old

An assistant skill and the Dalo executable are separate installations. If the
task calls for setup, use an installation channel appropriate to the machine,
preferring an existing package manager or the user's choice. Check the official
[installation instructions](https://dalo.sh/docs/getting-started.html) and
[installer options](https://dalo.sh/install.md) for current platform support and
version selection. For an upgrade, identify the current executable and channel
before replacing it; do not introduce a competing installation.

Run the chosen installation when it is within the requested setup scope and
the host permits it. When installation is unavailable, finish the filesystem
assessment and provide the one concrete step needed to continue. Do not claim
to have installed Dalo or modified its store. After installation, check the
binary's actual version and help again.

## Store and targets

Initialize only a missing store, then connect only the intended targets:

```sh
dalo --store "$store" --dry-run --json init
dalo --store "$store" --json init
dalo --store "$store" --json target detect
dalo --store "$store" --dry-run --json target link codex
dalo --store "$store" --json target link codex
```

Here `codex` is an example; use the agent and path established by the inventory.
An installed agent is not automatically a request to manage it. Linking records
the target; it does not adopt existing content. Continue with migration if that
directory is populated.

One target ID currently holds one path. Reusing it with another path changes
its existing assignment. For skills that belong to a repository, use Dalo's
project definition instead of redirecting the global store into the repository.
If `dalo-project.toml` exists, preview a pinned selection with `dalo project add`,
review its source, commit, skills, and targets, then apply the exact commit and
run `dalo install`. For an existing project source, use `dalo project update` to
review the new pin and inventory before applying it, then run `dalo install`;
changed content remains subject to local approval and blocking audits. If the
project has no definition, initialize it explicitly
with `dalo --project . init` first. The declaration is committed; the `.dalo`
store, approvals, and generated links remain local. Do not edit a repository's
ignore rules unless that is part of the task. Explicit store overrides bypass
project discovery; if the task is still project-scoped, pass `--project <root>`
to keep that scope explicit. See
[project installations](https://dalo.sh/docs/projects.html) and
[agent integration](https://dalo.sh/docs/agents.html).

## Sources and first sync

For a new conversational setup, check `dalo assistant install --help`. A
supporting binary carries the complete assistant; no second skill manager or
source download is needed:

```sh
dalo --store "$store" --dry-run --json assistant install
dalo --store "$store" --json assistant install
```

This prepares `local/skills/dalo` for the ordinary sync below. If an assistant
already comes from another installer or source, keep that ownership intact;
do not install a competing copy or overwrite its target. Updating an existing
bundled assistant changes what its current target symlinks read immediately.
The command blocks on local modifications. An older binary can keep using the
standalone skill; follow the official assistant guide if an upgrade is needed.

Use `source add` for an explicitly trusted team repository. It tracks the
repository by default and activates eligible skills. Supported `--ref` and
`--subpath` options can pin and scope a team source. Use `source add-catalog`
for third-party collections, then inspect, select, review, and narrowly approve
the requested skills. Do not treat a public URL as a trusted team source simply
to skip selection or approval.

Check existing source IDs and normalized origins before adding anything. Reuse
a matching source when its scope and update policy fit. Prefer selectors
reported by `source inspect` over guessed directory names. An add-catalog dry
run cannot list available skills because it does not clone the repository.

Preview the whole-store `sync`, check that its effects fit the request, apply it,
and verify the actual reports and links as described in `SKILL.md`. Do not enable periodic autosync,
tools, hooks, or instruction packs as a side effect of basic skill setup.

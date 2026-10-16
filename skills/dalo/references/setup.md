# Set up Dalo through the current agent

Complete the inventory first. An existing store should enter maintenance or
migration; do not recreate it as a shortcut.

## Binary missing or too old

A binary-only installation completes bundled skill setup on the first normal
CLI invocation; it verifies the matching packaged content on later use. Project
`install` keeps it project-local. If the
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

Normal use initializes a missing store for the packaged companion automatically.
Use explicit initialization when needed, and connect only the intended targets:

```sh
dalo --store "$store" --dry-run --json init
dalo --store "$store" --json init
dalo --store "$store" --json target detect
dalo --store "$store" --dry-run --json target link codex
dalo --store "$store" --json target link codex
```

Here `codex` is an example; use the agent and path established by the inventory.
An installed agent is not automatically a request to manage it. Linking records
the target and delivers only the bundled assistant on current releases; it does
not adopt other existing content. Continue with migration if that
directory is populated.

One target ID currently holds one path. Reusing it with another path changes
its existing assignment. For skills that belong to a repository, use Dalo's
project definition instead of redirecting the global store into the repository.
If `dalo-project.toml` exists, preview a pinned selection with `dalo project add`,
review its source, commit, skills, and targets, then apply the exact commit and
run `dalo install`. For an existing project source, use `dalo project update` to
review the new pin and inventory before applying it, then run `dalo install`;
changed content remains subject to blocking audits and, unless the declaration
sets `approval = "declaration"`, to local approval. If the
project has no definition, initialize it explicitly
with `dalo --project . init` first. The declaration is committed; the `.dalo`
store, approvals, and generated links remain local. Do not edit a repository's
ignore rules unless that is part of the task. Explicit store overrides bypass
project discovery; if the task is still project-scoped, pass `--project <root>`
to keep that scope explicit. See
[project installations](https://dalo.sh/docs/projects.html) and
[agent integration](https://dalo.sh/docs/agents.html).

## Sources and first sync

For a new conversational setup, inspect `dalo assistant status --json` after
initialization and target linking. Current releases deliver the embedded skill
without a separate install command, source download, or whole-store sync.
Respect `DALO_ASSISTANT_CHECK=never` if automatic setup was disabled. Older
supporting releases and explicitly requested recovery can use:

```sh
dalo --store "$store" --dry-run --json assistant install
dalo --store "$store" --json assistant install
```

Explicit installation only prepares `local/skills/dalo`; preview ordinary sync
before delivering it. Keep externally installed or customized assistants intact.
Updating an unmodified bundle changes existing target symlinks immediately;
never remove a receipt to bypass local-change protection.

Use `source add` for an explicitly trusted team repository. It tracks the
repository by default and activates eligible skills. Supported `--ref` and
`--subpath` options can pin and scope a team source. Use `source add-catalog`
for third-party collections, then inspect, select, review, and narrowly approve
the requested skills. Do not treat a public URL as a trusted team source simply
to skip selection or approval.

Before approving a selected skill, show its `compatibility` text and any
binaries it declares. Skill approval leaves a declared binary `pending_approval`;
`dalo approve binary <identity>` is a separate step that downloads and verifies
the host asset, so take it only when the user wants that skill's full
functionality on this machine.

Check existing source IDs and normalized origins before adding anything. Reuse
a matching source when its scope and update policy fit. Prefer selectors
reported by `source inspect` over guessed directory names. An add-catalog dry
run cannot list available skills because it does not clone the repository.

Preview the whole-store `sync`, check that its effects fit the request, apply it,
and verify the actual reports and links as described in `SKILL.md`. Do not enable periodic autosync,
tools, hooks, or instruction packs as a side effect of basic skill setup.

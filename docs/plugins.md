# Portable Plugins, Tools, and Hooks

A plugin is a passive package. It groups skills, canonical agents, and
instruction packs that belong together, and it may declare a narrowly typed
local executable and a hook that binds it to a provider event.

Nothing in a plugin is active by itself. Selecting a plugin resolves intent
only: it never grants a skill or agent approval, never enables instructions,
and never runs a tool. Every executable boundary is a separate, exact approval
that is bound to a hash, and any change to the contract invalidates it.

This guide walks the whole path. For field-by-field rules, see the
[`PLUGIN.toml` reference](reference.md#plugintoml-portable-plugins-tools-and-hooks).
The cross-implementation
[Portable Agent Packages specification](https://dalo.sh/spec/0.1/) is an
experimental draft, with a downloadable schema and an
[evidence-based compatibility matrix](https://dalo.sh/spec/0.1/compatibility.html).

## Validate a source before anyone adds it

Authors can check a source without a personal store:

```sh
dalo plugin validate ./my-source
dalo --json plugin validate ./my-source
dalo --json plugin validate ./my-source --source-id company
```

Validation applies the production discovery and dependency rules, reads package
files, and reports contract errors and unresolved references. It never runs
handlers, installs dependencies, grants trust, or changes provider files.
Unqualified references resolve inside the supplied source; `--source-id`
defaults to `validation` and should be set when the package uses explicit
references such as `skill:company:review`.

## The package

A package lives at `plugins/<name>/PLUGIN.toml` inside a source. The top-level
schema is closed: unknown fields are rejected.

```toml
schema_version = 1

[plugin]
name = "review-workflow"
description = "Shared review behavior across supported agents."

[[plugin.members]]
ref = "skill:review"
requirement = "required"

[[plugin.members]]
ref = "agent:reviewer"
requirement = "optional"
[plugin.members.fallback]
kind = "inline"
skill = "skill:review"

[[plugin.members]]
ref = "instruction:engineering-defaults"
requirement = "recommended"
```

Members carry `ref`, `requirement` (`required`, `optional`, or `recommended`),
and an optional `fallback`. Members may reference skills, agents, or
instructions; `recommended` is only valid for instructions. A fallback is only
valid for an agent member, uses `kind = "inline"`, names a skill in `skill`,
and that skill must also be a required member. Dependencies between packages
use `[[plugin.requires]]` with a `ref` and a `required` or `optional`
`requirement`.

## Select a plugin

The source root `dalo.toml` can select a package for everyone who uses that
source:

```toml
[source]
id = "company"

[selection]
plugins = [{ ref = "company:review-workflow", requirement = "required" }]
```

A local selection is additive on top of that stack:

```sh
dalo plugin list
dalo plugin show company:review-workflow
dalo plugin select company:review-workflow
dalo plugin unselect company:review-workflow
```

`plugin unselect` removes only that local origin and never edits the
source-authored stack. `plugin decline` keeps the selected intent but blocks the
plugin with a local policy record; it needs a unique lower-kebab `--rule-id` and
a non-empty `--reason`.

## Preview before anything is written

```sh
dalo plan
dalo plan --target codex --json
```

The plan reports every linked target without writing provider state. A
recommended instruction stays inactive until the explicit
`dalo instructions enable` flow has completed.

## Review the whole closure at once

```sh
dalo plugin review company:review-workflow
dalo --dry-run plugin review company:review-workflow
dalo --json plugin review company:review-workflow
```

`plugin review` turns the selected plugin and its dependency closure into one
coherent session. It still asks separately for each pending skill, agent, tool,
and hook contract, shows exact hashes and Codex/Claude mappings, and then asks
once more before committing that explicit set with one atomic approval-ledger
write. It never creates plugin-, source-, author-, organization-, or wildcard
trust.

`--json` and `--dry-run` are strictly read-only: they never prompt, stage
executable bytes, run external reviewers, or write provider targets. Use the
individual `dalo approve ...` commands when reviewing just one known boundary,
when revoking, or when a blocking skill audit needs an explicit `--accept-risk`
reason.

## Tools

A package may declare a narrowly typed local executable. Discovery, `status`,
`doctor`, `plan`, and `sync --dry-run` only inventory and hash it; they never
run it. Execution trust is a separate, exact contract approval.

```toml
[[tool]]
schema_version = 1
id = "detector"
entry = "tools/detect.py"
runtime = "python"
runtime_version = ">=3.11"
platforms = ["macos", "linux"]
argv = ["--path", "${input.path}"]
files = ["tools/rules.json"]
cwd = "tool_root"
env = ["DALO_LOG"]
capabilities = ["filesystem_read"]
availability = "required"

[[tool.inputs]]
name = "path"
type = "path"
required = true
```

`schema_version`, `id`, `entry`, `runtime` (`executable`, `python`, or `node`),
`argv`, `cwd` (`tool_root`), and `availability` (`required` or `optional`) are
required. `runtime_version`, `platforms` (`macos`, `linux`), `inputs`, `files`,
`env`, and `capabilities` (`filesystem_read`, `filesystem_write`, `subprocess`,
or `network`) are optional. Inputs are closed `name`, `type` (`string`, `path`,
`integer`, `boolean`), and optional `required` records. Paths must be
plugin-root-relative and stay inside the plugin-owned closure.

```sh
dalo tool list
dalo tool show company:review-workflow#tool:detector
dalo tool audit company:review-workflow#tool:detector
dalo approve tool company:review-workflow#tool:detector
dalo approve revoke tool company:review-workflow#tool:detector
```

Approval records include the deterministic tool-contract hash. Approved bytes
are atomically promoted below Dalo's immutable content-addressed tool root;
changing the entry, referenced files, runtime, input/argv contract, environment,
working directory, platform, availability, or capabilities requires approval
again. An unrelated plugin README change retains the approval and is still
visible through the changed whole-package provenance hash.

## Hooks

An approved tool can be bound to a portable hook through a second, independent
approval. The binding is typed and cannot change the tool-owned argv template:

```toml
[[hook]]
schema_version = 1
id = "check-shell"
tool = "detector"
subject = "tool_call"
phase = "before"
effect = "allow_deny"
requirement = "required"
timeout_ms = 2000
failure_policy = "fail_closed"
retry = "never"
error_visibility = "model_and_user"
blocking_scope = "matched_event"
bindings = [{ input = "path", field = "session.cwd" }]
matcher = { tool_names = ["Bash"] }
```

```sh
dalo hook list
dalo hook show company:review-workflow#hook:check-shell
dalo approve hook company:review-workflow#hook:check-shell
dalo sync --dry-run
dalo sync
dalo approve revoke hook company:review-workflow#hook:check-shell
```

The hook approval covers the exact tool hash, event, effect, matcher, typed
bindings, timeout, failure behavior, and blocking scope. Sync projects only
selected and independently approved hooks into structurally owned Codex or
Claude entries. Provider event JSON travels over stdin to Dalo's dispatcher;
it is never interpolated into a shell command. Native files use compare-and-swap
and preserve foreign settings, while `status` and `doctor` report disabled,
managed-only, unverified, drifted, conflicted, and revoked states separately.

The full list of admitted subjects, phases, effects, failure policies, and
event fields is in the
[hook reference](reference.md#plugintoml-portable-plugins-tools-and-hooks).

## Native provider packages

Selected coherent plugins are rendered as one independently owned native
package per linked provider. The same `company:review-workflow` selection
produces a Codex package with `.codex-plugin/plugin.json` and a Claude package
with `.claude-plugin/plugin.json`; both contain supported `skills/`, while
Claude can additionally contain compiled `agents/`. Codex agents and standing
instruction packs remain explicit external projections because those concepts
do not belong in the Codex plugin layout. Dalo records every omission,
component fingerprint, adapter baseline, immutable artifact hash, and owned
provider path in `plugins/state.json`.

Claude's package is linked into its configured skills directory, where
skills-directory plugin loading can discover it. The Codex package is kept
below the Codex configuration root at `plugins/dalo/<native-name>`; Dalo does
not silently rewrite a user's marketplace catalog or plugin enablement.
`dalo plan`, `status`, `doctor`, and `sync --dry-run` show both paths and every
component outcome before mutation. Required tools and hooks must retain their
separate exact approvals or the package projection is blocked. Ordinary
harness-neutral skills still use the existing byte-identical direct symlinks.

## Related

- [Command reference](reference.md) — every flag, JSON shape, and diagnostic code
- [Security overview](security.md) — trust boundaries and the stated limits
- [Compatibility and stability](compatibility.md) — what the 1.x contract covers
- [Troubleshooting and FAQ](troubleshooting.md) — the command that clears each finding

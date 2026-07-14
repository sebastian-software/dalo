# Dalo User Reference

This is the user-facing reference for scripting Dalo and for understanding the files Dalo writes. It documents the current CLI and persisted schemas.

## Store Resolution

Dalo chooses the store path in this order:

1. `--store <PATH>`
2. `DALO_STORE`
3. `~/.dalo`

Relative store paths are resolved against the current working directory. `~` is expanded when used at the start of a path.

## Environment Variables

| Variable | Purpose |
| --- | --- |
| `DALO_STORE` | Override the default store path; `--store` takes precedence. |
| `DALO_GIT_TIMEOUT_SECS` | Positive timeout in seconds for every Git subprocess. Invalid or zero values use the built-in defaults. |
| `NO_COLOR` | Disable ANSI color output when set. |

For installation variables, see the [README installation section](../README.md#installation)
and the [npm launcher README](../npm/README.md): `DALO_VERIFY`,
`DALO_LINUX_LIBC`, `DALO_TARGET`, `DALO_INSTALL_DIR`, `DALO_VERSION`, and
`DALO_CACHE_DIR`.

## Global Flags

Global flags can be placed before or after the command.

| Flag | Meaning |
| --- | --- |
| `--store <PATH>` | Use a store other than the resolved default. |
| `--json` | Emit machine-readable JSON for commands that support structured output. |
| `--yes` | Reserved for future safe interactive prompts. It is currently a no-op and never implies `--replace`, creates commits, or grants new approvals. |
| `--dry-run` | Plan supported mutating operations without writing files, cloning, linking, or changing locks. Read-only commands ignore it. |
| `-h`, `--help` | Print command help. |
| `-V`, `--version` | Print the installed version. |

## Command Reference

### `dalo init`

Initialize the store layout, default config, local Git source, lock/state files, and approvals file.

Examples:

```sh
dalo init
dalo --store /tmp/dalo-demo init
dalo --json --dry-run init
```

JSON output shape: `InitReport`.

### `dalo target detect`

List known agent targets, their default paths where known, whether those paths exist, and whether they are linked in Dalo state.

Examples:

```sh
dalo target detect
dalo --json target detect
```

Built-in target IDs:

| ID | Default path | Support |
| --- | --- | --- |
| `codex` | `~/.agents/skills` | supported |
| `claude` | `~/.claude/skills` | supported |
| `openclaw` | `~/.agents/skills` | supported |
| `hermes` | `~/.hermes/skills` | supported |
| `generic` | none, path required | supported |
| `cursor` | none | experimental |
| `opencode` | none | experimental |

JSON output shape: `TargetDetectReport`.

### `dalo target link <target> [path]`

Record a target materialization directory. The `generic` target requires an explicit path. Known targets can use their default path or an explicit override.

Examples:

```sh
dalo target link codex
dalo target link generic ./tmp/agent-skills
dalo --dry-run target link claude ~/.claude/skills
```

JSON output shape: `TargetLinkReport`.

### `dalo target unlink <target>`

Disable a target in state. This does not delete skills from the target directory.

Examples:

```sh
dalo target unlink codex
dalo --json target unlink generic
```

JSON output shape: `TargetUnlinkReport`.

### `dalo source add <id> <git-url>`

Add a trusted team source, clone it into `sources/<id>/checkout`, and configure it with `update_policy = "track"`. Source IDs must be a single path component using only letters, digits, `.`, `_`, and `-`; `.` and `..` are rejected.

Examples:

```sh
dalo source add platform git@github.com:example/platform-skills.git
dalo --dry-run source add team https://github.com/example/team-skills.git
```

JSON output shape: `SourceAddReport`.

### `dalo source add-catalog <id> <git-url>`

Add an untrusted catalog source, clone it into `sources/<id>/checkout`, and
configure it with `update_policy = "pin"`. Catalog skills are offers; nothing
from a catalog becomes active until selected and approved.

Examples:

```sh
dalo source add-catalog public https://github.com/example/skill-catalog.git
dalo source inspect public
dalo source select public review-helper
```

JSON output shape: `SourceConfig`.

### `dalo source list`

List configured sources in priority order.

Examples:

```sh
dalo source list
dalo --json source list
```

JSON output shape: `SourceListReport`.

### `dalo source priority <id> <priority>`

Change a source priority. Lower numbers win during resolution. The local source priority is fixed and cannot be changed.

Examples:

```sh
dalo source priority platform 10
dalo --dry-run source priority platform 20
```

JSON output shape: `SourcePriorityReport`.

### `dalo source inspect <id>`

Inspect a catalog source and list available candidate skills, including ID, slot name, path, description, dependencies, and selection status.

Examples:

```sh
dalo source inspect public
dalo --json source inspect public
```

JSON output shape: `CatalogInspectReport`.

### `dalo source select <id> <skill>...`

Select catalog skills by stable frontmatter ID, `<source-id>:<slot>` reference, or slot name. Selection writes `config.toml` and updates `source-lock.toml` with the pinned commit and inventory snapshot.

Examples:

```sh
dalo source select public review-helper
dalo source select public review-helper formatter
dalo source select public --unselect formatter
dalo --dry-run source select public review-helper
```

JSON output shape: `CatalogSelectReport`.

### `dalo source refresh <id>`

Fetch a catalog source and compare the upstream inventory with the pinned
inventory snapshot. This is read-only for pins and selections. `--check` exits
non-zero when selected skills drifted upstream. Advancing the pin is not
implemented yet.

Examples:

```sh
dalo source refresh public
dalo source refresh public --check
dalo --json source refresh public
```

JSON output shape: `CatalogDrift`.

With `--check`, the command exits with code 1 when a selected skill changed,
moved, or was removed upstream. New unselected offerings remain informational.

### `dalo source remove <id>`

Remove a team or catalog source as one coordinated change. Dalo first reconciles
only links it owns, then removes the source from `config.toml`, its catalog lock
entry (when present), and approvals qualified with that source ID. The source
checkout is removed after the durable store state is committed. Use
`--keep-checkout` to retain it for manual inspection. The built-in `local`
source cannot be removed. A retained checkout must be moved or removed before
the same source ID can be added again.

Examples:

```sh
dalo --dry-run --json source remove platform
dalo source remove public
dalo source remove public --keep-checkout
```

Both dry-run and real `--json` output use `SourceRemoveReport`. It lists
deactivated skills, the operation kind for every reconciled owned link, and any
non-fatal checkout cleanup warnings. Dry-run does not modify config, locks,
approvals, checkouts, or targets.

### `dalo status`

Show source scans, linked targets, active skills, pending approvals, unlinked skills,
user-lock drift, unmanaged target skills, instruction packs, topic overlaps, and
instruction block drift.

Examples:

```sh
dalo status
dalo status --check
dalo --json status
```

JSON output shape: `StatusReport`.

`--check` exits with code 1 for unresolved source scans, inventory warnings,
pending approvals, blocked required closure, missing targets for active skills,
lock drift, unmanaged blockers, or instruction-block drift. It keeps the full
report on stdout for JSON consumers.

### `dalo sync`

Refresh clean tracking team sources, resolve the desired skill set, materialize owned symlinks into linked targets, and write `lock.toml`. Dirty tracking sources block refresh. Dalo does not overwrite unmanaged real directories or foreign symlinks.

Examples:

```sh
dalo sync
dalo sync --check
dalo --dry-run sync
dalo --json sync
```

JSON output shape: `SyncReport`.

`sync --check` still renders the report, then exits with code 1 when materialization
is blocked or incomplete, including pending approvals, resolution diagnostics,
degraded sources, blocked operations, or active skills without linked targets.

An enabled source is **degraded** when Dalo cannot safely scan it, for example
because its checkout is missing or unreadable, a tracking refresh failed, or
inventory warnings make removals ambiguous. Dalo preserves recorded owned links
from that source instead of treating the incomplete scan as a deletion. A normal
`sync` can still apply unrelated safe work and exits successfully; `sync --check`
exits with code 1 until the source is healthy. Restore or re-clone the checkout,
or remove the source with `dalo source remove <id>`. Do not adopt or delete the
preserved target link as if it were an unmanaged conflict.

Lock drift for local and team sources is commit-based. Uncommitted working-tree
edits do not change the recorded commit, and materialized symlinks expose those
live edits directly. Catalog selections are the source kind with content and
metadata fingerprints for upstream drift checks.

### `dalo adopt <skill> [--replace]`

Copy an unmanaged target skill into `local/skills/<slot>`. With `--replace`, Dalo replaces the original unmanaged directory with an owned symlink after copying. Without `--replace`, the original directory remains untouched.

The `<skill>` argument can be a slot name, a disambiguating path, or an ID reported by `status` or `resolve list`.

Examples:

```sh
dalo adopt review-helper
dalo adopt review-helper --replace
dalo --dry-run adopt /path/to/target/review-helper
```

JSON output shape: `AdoptReport`.

### `dalo resolve list`

List repairable unmanaged skills, target scan warnings, and recorded owned symlinks.

Examples:

```sh
dalo resolve list
dalo --json resolve list
```

JSON output shape: `ResolveListReport`.

### `dalo resolve adopt <id> [--replace]`

Adopt an unmanaged skill by an ID returned from `resolve list`. Behavior matches `dalo adopt`.

Examples:

```sh
dalo resolve adopt review-helper
dalo resolve adopt review-helper --replace
```

JSON output shape: `AdoptReport`.

### `dalo resolve keep <id>`

Protect an unmanaged target skill so Dalo reports it as intentionally unmanaged. Protection is stored by logical target and slot, follows later target-path updates, and turns the sync conflict into a non-failing `keep` operation. An explicit `resolve adopt --replace` remains an override.

Examples:

```sh
dalo resolve keep review-helper
dalo --dry-run resolve keep review-helper
```

JSON output shape: `KeepReport`.

### `dalo resolve unkeep <target>:<slot>`

Remove protection from one target slot. A bare slot name removes matching protection across targets.

Examples:

```sh
dalo resolve unkeep claude:review-helper
dalo --dry-run resolve unkeep review-helper
```

JSON output shape: `UnkeepReport`.

### `dalo resolve remove-owned <id>`

Remove a recorded owned symlink by ID. If the recorded path is already missing, Dalo drops the stale state record. If a real entry exists at that path, Dalo blocks removal.

Examples:

```sh
dalo resolve remove-owned claude:review-helper
dalo --json resolve remove-owned claude:review-helper
```

JSON output shape: `RemoveOwnedReport`.

### `dalo doctor`

Run read-only diagnostics for store layout, config, state, lock, approvals, Git, GitHub CLI, targets, owned symlinks, dirty sources, pending approvals, required closures, instruction packs, and cloud-synced target paths.

Examples:

```sh
dalo doctor
dalo doctor --check
dalo --json doctor
```

JSON output shape: `DoctorReport`.

`--check` exits with code 1 when the report contains an error finding. Warnings
remain report-only so CI can choose its own warning policy.

### `dalo approve`

Grant, list, and revoke approval records without editing `approvals.toml`.
Every skill, author, and organization value is source-qualified, so an approval
cannot accidentally apply to a different source with the same name.

```sh
dalo approve list
dalo approve skill public:review-helper
dalo approve source team
dalo approve author public:maintainers
dalo approve org public:example-org
dalo approve revoke skill public:review-helper
```

Approval writes support `--dry-run` and `--json`. A pending skill shown by
`status` can be approved narrowly with `dalo approve skill <source:skill>`.

JSON output shapes: `ApprovalsFile` for `list`; `ApprovalReport` for mutations.

### `dalo instructions enable <pack> <file>`

Render a local instruction pack from `local/instructions/<pack>.md` into an instruction file as a managed block. The target file is created if missing. Enabling the same pack again is idempotent and refreshes the block.

Examples:

```sh
dalo instructions enable team-style ~/.codex/AGENTS.md
dalo --dry-run instructions enable team-style ./AGENTS.md
```

JSON output shape: `InstructionPackReport`.

### `dalo instructions disable <pack> <file>`

Remove a pack's managed block from an instruction file and remove its active lock entry.

Examples:

```sh
dalo instructions disable team-style ~/.codex/AGENTS.md
dalo --json instructions disable team-style ./AGENTS.md
```

JSON output shape: `InstructionPackReport`.

### `dalo instructions list`

List active instruction packs recorded in `lock.toml`.

Examples:

```sh
dalo instructions list
dalo --json instructions list
```

JSON output shape: array of `LockedInstructionPack`.

### `dalo completions <shell>`

Generate shell completions to stdout. Supported shell names are provided by `clap_complete`, including `bash`, `zsh`, and `fish`.

Examples:

```sh
dalo completions zsh > _dalo
dalo completions bash > dalo.bash
dalo completions fish > dalo.fish
```

Release archives include generated completions. This command is also available for Cargo installs and local shell setup.

### `dalo manpage`

Generate the `dalo(1)` man page to stdout from the same Clap command definition used for `--help`.

Example:

```sh
dalo manpage > dalo.1
```

Release archives include the generated man page. This command is also available for Cargo installs and local documentation setup.

## Exit Codes

Dalo uses a small scripting contract:

| Code | Name | Meaning |
| --- | --- | --- |
| `0` | success | Command completed. |
| `1` | expected failure | User-actionable input/state problem, such as unknown source, unknown target, unsupported schema, parse error, a failed explicit check, or adoption destination already existing. |
| `2` | usage error | Invalid arguments or flags from Clap. This output is plain text even with `--json`. |
| `3` | unsafe state | Dalo refused to mutate because the state needs human attention, such as a dirty source, active store lock, or malformed instruction block. |
| `4` | environment problem | Dependency, path, Git, filesystem, or external command problem. |

Scripts should treat `3` differently from `1`: it means Dalo intentionally stopped before touching state that may require review.

## JSON Output Shapes

`--json` prints one JSON value to stdout on success. Runtime errors are printed to stderr as `{"error":{"code":"...","message":"..."}}` and keep the same exit code as text output. Path fields serialize as strings. Enum fields serialize as `snake_case` unless noted. Commands that mutate and support `--dry-run` include a `dry_run` boolean in their report.

| Command | Shape | Important fields |
| --- | --- | --- |
| `init` | `InitReport` | `store`, `dry_run`, `operations[]` with `action`, `path`, `status` |
| `target detect` | `TargetDetectReport` | `targets[]` with `id`, `name`, `support`, `path`, `exists`, `linked` |
| `target link` | `TargetLinkReport` | `target_id`, `path`, `canonical_path`, `status`, `created_dir` |
| `target unlink` | `TargetUnlinkReport` | `target_id`, `status` |
| `source add` | `SourceAddReport` | `source`, `dry_run` |
| `source add-catalog` | `SourceConfig` | `id`, `kind`, `path`, `priority`, `enabled`, `trusted`, `url`, `update_policy`, `selection` |
| `source list` | `SourceListReport` | `sources[]` |
| `source priority` | `SourcePriorityReport` | `source`, `dry_run` |
| `source inspect` | `CatalogInspectReport` | `source_id`, `candidates[]` |
| `source select` | `CatalogSelectReport` | `source_id`, `selected[]`, `dry_run` |
| `source refresh` | `CatalogDrift` | `source_id`, `pinned_commit`, `upstream_commit`, `outcomes[]` |
| `source remove` | `SourceRemoveReport` | `source_id`, `checkout_path`, `kept_checkout`, `removed_approvals`, `removed_catalog_lock`, `reconciled_links[]`, `deactivated_skills[]`, `cleanup_warnings[]`, `affected_paths[]`, `dry_run` |
| `status` | `StatusReport` | `store`, `sources[]`, `targets[]`, `inventory_warnings[]`, `resolution`, `lock`, `unmanaged_skills[]`, `target_warnings[]`, `instruction_packs[]`, `instruction_pack_overlaps[]`, `instruction_block_drifts[]` |
| `sync` | `SyncReport` | `store`, `dry_run`, `linked_targets`, `operations[]` |
| `approve list` | `ApprovalsFile` | `schema_version`, `approvals[]` |
| `approve skill` / `source` / `author` / `org` / `revoke` | `ApprovalReport` | `scope`, `value`, `action`, `dry_run` |
| `adopt` / `resolve adopt` | `AdoptReport` | `slot_name`, `source_path`, `local_path`, `copy`, `replacement` |
| `resolve list` | `ResolveListReport` | `unmanaged_skills[]`, `target_warnings[]`, `owned_skills[]` |
| `resolve keep` | `KeepReport` | `skill`, `existing`, `dry_run` |
| `resolve unkeep` | `UnkeepReport` | `selector`, `removed[]`, `dry_run` |
| `resolve remove-owned` | `RemoveOwnedReport` | `id`, `link_path`, `status` |
| `doctor` | `DoctorReport` | `store`, `findings[]`, `summary` |
| `instructions enable` / `disable` | `InstructionPackReport` | `pack_id`, `target`, `action`, `dry_run` |
| `instructions list` | `LockedInstructionPack[]` | `pack_id`, `target`, `source_id`, optional `commit`, optional `version` |

Common status values:

| Field | Values |
| --- | --- |
| `TargetSupport` | `supported`, `experimental` |
| `TargetLinkStatus` | `planned`, `linked`, `updated`, `existing` |
| `TargetUnlinkStatus` | `planned`, `unlinked`, `missing` |
| `MaterializeOperationKind` | `create`, `relink`, `remove`, `drop_record`, `conflict`, `keep`, `noop` |
| `MaterializeOperationStatus` | `planned`, `applied`, `existing`, `blocked` |
| `AdoptCopyStatus` | `planned`, `copied`, `existing` |
| `AdoptReplacementStatus` | `planned`, `replaced`, `skipped`, `protected` |
| `RemoveOwnedStatus` | `planned`, `removed`, `dropped_missing`, `blocked_real_entry` |
| `InstructionBlockDriftKind` | `missing`, `malformed`, `stale`, `source_missing` |
| `CatalogDrift.code` | `new_available`, `selected_changed`, `selected_moved`, `selected_removed` |
| `TargetScanWarningCode` | `unreadable_target_dir` |

`DoctorFinding` uses:

```json
{
  "severity": "warning",
  "code": "pending_approval",
  "message": "human-readable diagnostic",
  "next_command": "optional next command"
}
```

Doctor severities are `error`, `warning`, `info`, and `ok`.

Resolution diagnostics use these codes when present in `status.resolution.diagnostics`: `pending_approval`, `local_override`, `shadowed`, `required_expanded`, `cross_source_require`, and `required_blocked`. For recovery steps, see [Troubleshooting and FAQ](troubleshooting.md).

## Store Layout

After `dalo init`, the store contains:

| Path | Purpose |
| --- | --- |
| `config.toml` | User-authored config: settings and source list. |
| `lock.toml` | Resolved user lock written by `sync` and instruction commands. |
| `state.toml` | Internal target/materialization/protection state. |
| `approvals.toml` | Local approval records. |
| `source-lock.toml` | Catalog source pins, selections, and inventory snapshots. |
| `.lock` | Temporary coarse lock file while mutating commands run. |
| `local/skills/` | Local private skill directories. |
| `local/instructions/` | Local instruction pack Markdown files. |
| `sources/<id>/checkout/` | Team and catalog Git checkouts. |

Dalo rejects unsupported schema versions in persisted TOML files.

## `config.toml`

Schema version: `version = 1`.

Example:

```toml
version = 1

[settings]
autosync = false
sync_interval = "hourly"

[[sources]]
id = "local"
kind = "local"
path = "/Users/alex/.dalo/local"
priority = 0
enabled = true
trusted = true

[[sources]]
id = "team"
kind = "team"
path = "/Users/alex/.dalo/sources/team/checkout"
priority = 10
enabled = true
trusted = true
url = "git@github.com:example/team-skills.git"
update_policy = "track"

[[sources]]
id = "public"
kind = "catalog"
path = "/Users/alex/.dalo/sources/public/checkout"
priority = 20
enabled = true
trusted = true
url = "https://github.com/example/catalog.git"
update_policy = "pin"
selection = ["review-helper"]
```

Fields:

| Field | Meaning |
| --- | --- |
| `settings.autosync` | Reserved scheduled-sync switch. |
| `settings.sync_interval` | Optional interval label. |
| `sources[].id` | Stable source ID. |
| `sources[].kind` | `local`, `team`, or `catalog`. |
| `sources[].path` | Local filesystem path for the source root or checkout. |
| `sources[].priority` | Lower numbers win. |
| `sources[].enabled` | Disabled sources are skipped by resolution. |
| `sources[].trusted` | Trusted sources are approved automatically. User-added catalog sources always start untrusted. |
| `sources[].url` | Git URL for team/catalog sources. URLs with embedded credentials are rejected; use SSH or a credential helper. |
| `sources[].branch` | Optional branch label. |
| `sources[].update_policy` | Usually `track` for team sources and `pin` for catalog sources. |
| `sources[].selection` | Catalog selections by stable ID, source ref, path, or slot name. Empty for local/team sources. |

Unknown fields are rejected.

## `approvals.toml`

Schema version: `schema_version = 1`.

Example:

```toml
schema_version = 1

[[approvals]]
scope = "source"
value = "team"

[[approvals]]
scope = "skill"
value = "team:review-helper"

[[approvals]]
scope = "author"
value = "team:platform-team"
```

Approval scopes:

| Scope | Value | Matches |
| --- | --- | --- |
| `skill` | `<source-id>:<slot>` or `<source-id>:<stable-id>` | One skill from one source. Bare slot names and bare stable IDs do not match. |
| `source` | `<source-id>` | Every skill from that source. |
| `author` | `<source-id>:<owner>` | Skills from that source whose `owners` frontmatter contains that owner. |
| `org` | `<source-id>:<owner>` | Same matching behavior as `author`; the scope is a policy label. |

Local skills and skills from `trusted = true` sources are approved automatically. Non-interactive commands can use existing approvals but never create new approvals.

Prefer `dalo approve` for all approval changes. The TOML format remains
documented for auditability and recovery, but does not need to be edited for
normal approval workflows.

## `lock.toml`

Schema version: `schema_version = 1`.

This file is Dalo's resolved user lock. `sync` rewrites source snapshots, active skills, pending approvals, unlinked skills, and target materialization summaries. Instruction commands preserve and update active instruction-pack entries.

Top-level fields:

| Field | Meaning |
| --- | --- |
| `sources[]` | Source ID, kind, path, and optional commit. |
| `active_skills[]` | Skills that should be linked into targets. |
| `pending_approval_skills[]` | Skills blocked by approval. |
| `unlinked_skills[]` | Managed skills not linked, with a reason such as shadowing. |
| `target_materializations[]` | Last sync operations by target path. |
| `active_instruction_packs[]` | Instruction packs rendered into instruction files. |

Important record fields:

| Record | Fields |
| --- | --- |
| `LockedSource` | `id`, `kind`, `path`, optional `commit` |
| `LockedSkill` | `source_ref`, `slot_name`, optional `id`, `source_id`, `source_kind`, optional `reason` |
| `LockedTargetMaterialization` | `link_path`, optional `desired_path`, `kind`, `status`, optional `reason` |
| `LockedInstructionPack` | `pack_id`, `target`, `source_id`, optional `commit`, optional `version` |

## `state.toml`

Schema version: `schema_version = 1`.

This is internal state. Users normally inspect it through `target detect`, `status`, `sync`, and `resolve list`.

Top-level fields:

| Field | Meaning |
| --- | --- |
| `targets[]` | Logical target links with `id`, `path`, `canonical_path`, and `enabled`. |
| `materialization_dirs[]` | Canonical physical directories and the logical target IDs sharing each directory. |
| `owned_skills[]` | Symlinks Dalo owns: `target_id`, `slot_name`, `link_path`, `store_path`. |
| `protected_skills[]` | Unmanaged target slots kept by the user: `target_id`, `slot_name`. Legacy path-based entries migrate on read. |

Unknown fields in this internal state model are retained across reads and writes for downgrade safety after additive changes. Breaking changes still require a schema-version bump. User-authored configuration remains strict.

## `source-lock.toml`

Schema version: `schema_version = 3`.

This file stores catalog pins and inventory snapshots. It is written by catalog selection and read by drift checks.

Top-level fields:

| Field | Meaning |
| --- | --- |
| `catalogs[]` | One `CatalogLock` per pinned catalog source. |
| `catalogs[].source_id` | Catalog source ID. |
| `catalogs[].commit` | Pinned commit used for the inventory snapshot. |
| `catalogs[].selected[]` | Selected skill references. |
| `catalogs[].inventory[]` | Pinned catalog entries. |

Inventory entry fields:

| Field | Meaning |
| --- | --- |
| `id` | Optional stable skill ID from frontmatter. |
| `slot_name` | Install slot name. |
| `path` | Skill directory path relative to the catalog root. |
| `content_hash` | Hash over skill directory content, executable file bits, and symlink metadata. |
| `metadata_hash` | Hash over parsed skill metadata. |
| `requires[]` | Declared same-source dependencies. |

## `SKILL.md` Frontmatter

Dalo discovers a skill by finding a directory containing `SKILL.md`. If the file starts with YAML frontmatter fenced by `---` lines, Dalo reads these fields:

```markdown
---
id: review-helper
name: review-helper
description: Review pull requests for behavior regressions
owners:
  - platform-team
tags:
  - review
requires:
  - base-style
---

# Review Helper
```

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | no | Stable skill identity. Use for catalog selections and source-qualified approvals. |
| `name` | no | Preferred slot name. Must be a safe slot name; otherwise Dalo falls back to the folder name and records an inventory warning. |
| `description` | no | Human-readable description shown in catalog inspection. |
| `owners[]` | no | Approval owner strings matched by `author` and `org` approvals. |
| `tags[]` | no | User metadata. |
| `requires[]` | no | Same-source or same-catalog dependencies. Required skills are expanded only when the closure is linkable and approved. |

If `name` is absent, the directory name is the slot name. Duplicate slot names within one source are warned and de-duplicated by resolver behavior.

## Instruction Packs

Instruction packs are Markdown files in `local/instructions/<id>.md` or
`<source>/instructions/<id>.md`. Dalo discovers both locations, but
`instructions enable` currently reads only local packs; source packs are
discovery-only. Pack IDs use the same safe token rule as source IDs: letters,
digits, `.`, `_`, and `-`, excluding `.` and `..`.

Dalo currently reads optional leading metadata lines from the first five lines:

```markdown
version: 1.0.0
topics: review, formatting

# Team Style

Use concise review comments.
```

`tags:` can be used instead of `topics:`. Topics are comma-separated and are used for advisory overlap warnings when multiple active packs share topics.

Managed blocks use these markers:

```markdown
<!-- dalo:start team-style -->
...
<!-- dalo:end team-style -->
```

Dalo only rewrites bytes inside the pack's own managed block. Content outside managed blocks is preserved.

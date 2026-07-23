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
| `DALO_OFFLINE` | Disable passive update checks when set to a truthy value. |
| `DALO_UPDATE_CHECK` | Set to `never` to disable passive update checks. |
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

## Update Notices

After a successful interactive command, Dalo checks for a newer GitHub release
at most once per 24 hours. The cached check uses a one-second network timeout and
never changes the command's exit status. Checks are skipped for `--json`, CI,
`DALO_OFFLINE=1`, and `DALO_UPDATE_CHECK=never`.

Dalo never modifies its own executable. If the installed version is outdated,
the notice recommends an upgrade command for Homebrew, npm/npx, mise, Cargo, or
the hosted installer. Launchers may set `DALO_INSTALL_CHANNEL` so the Rust binary
can preserve their installation context. Unknown installation methods receive a
link to the installation guide instead of a guessed command. Each newer version
is announced only once per user cache.

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

### `dalo team init <source-id>`

Create `dalo.toml` in the current team repository. Use `--repo <path>` on the
`team` command to target another checkout. Team-management commands do not read
or initialize the personal Dalo store, and they do not commit or push changes.
The global `--store` flag is accepted for uniformity but has no effect on `team`
commands; select the target repository with `--repo` instead. They write the
manifest in canonical TOML form; catalog mutations may normalize formatting and
do not preserve comments.

Examples:

```sh
dalo team init company --name "Company Skills"
dalo team --repo ../company-skills init company
dalo --dry-run team init company
```

An existing manifest with the same source ID is left unchanged. A different ID
is never overwritten. JSON output shape: `TeamManifestMutationReport`.

### `dalo team catalog add <id> <git-url-or-path> --version <revision>`

Add a pinned external skill set to the team manifest. Repeat `--skill` for
include/exclude filters; omitting it means all skills.

```sh
dalo team catalog add marketing https://github.com/coreyhaines31/marketingskills.git \
  --version 0123456789abcdef0123456789abcdef01234567 \
  --skill +copywriting \
  --skill +launch \
  --skill -seo-audit
```

Use `--priority <number>` to override the derived default. JSON output shape:
`TeamManifestMutationReport`.

### `dalo team catalog skills <id> [filter]...`

Replace the complete filter list. Calling the command without filters sets
`skills = []`, which means all skills.

```sh
dalo team catalog skills marketing +copywriting +launch -seo-audit
dalo team catalog skills marketing
```

JSON output shape: `TeamManifestMutationReport`.

### `dalo team catalog version <id> <revision>`

Change the requested commit, tag, or ref. The next `dalo sync` on each team
member's machine stages, audits, and pins the resolved commit before publishing
it.

```sh
dalo team catalog version marketing v2.0.0
```

JSON output shape: `TeamManifestMutationReport`.

### `dalo team catalog update <id> --from <ref>`

Resolve an upstream branch, tag, or ref in a temporary clone, compare the
currently declared version with the candidate inventory, and run deterministic
audits for the selected candidate skills. A successful real update writes the
exact candidate commit to `dalo.toml`; it never leaves the shared version as a
floating ref and never commits or pushes the repository.

```sh
dalo --dry-run team catalog update marketing --from main
dalo --json --dry-run team catalog update marketing --from v2
dalo team catalog update marketing --from main
dalo team catalog update marketing --from main \
  --accept-risk "reviewed pinned automation"
```

Dry-run performs network reads and temporary filesystem work but does not edit
the repository or personal Dalo store. A non-fast-forward candidate, a removed
selected skill, an invalid candidate selection, or a blocking audit finding
prevents the write. `--accept-risk <reason>` is required to be non-empty and
accepts only blocking security-audit findings from this exact candidate. It
does not bypass non-fast-forward updates, removed skills, invalid selections,
or other structural blockers. Each accepted audit report retains its
content-bound `risk_acceptance` scope hash; no personal store state is written.
JSON output shape: `TeamCatalogUpdateReport`, including
`old_version`, exact `old_commit` and `candidate_commit`, `outcomes[]`,
`audits[]`, `accepted_risk_reason`, `blocking_reasons[]`, `dry_run`, and
`updated`.

### `dalo team catalog remove <id>`

Remove the declaration. The next team-member sync removes generated source
state, approvals, owned links, and checkout state for that catalog.

JSON output shape: `TeamManifestMutationReport`.

### `dalo team show`

Print the source identity and catalog declarations from `dalo.toml`. Use
`--json` for `TeamManifestView`.

### `dalo target detect`

List known agent targets, their linked paths when configured (otherwise their
default paths where known), whether those paths exist, and whether they are
linked in Dalo state. The text output finishes with the next link command when
it finds an installed-but-unlinked agent, or with a generic-folder command when
no agent folder is present. `dalo target list` is an alias for this command.

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

When a target path does not exist yet, Dalo creates its missing directory
ancestors only after validating the state update. If persisting `state.toml`
fails, newly created empty ancestors are removed again; pre-existing directories
and any concurrent content are preserved.

JSON output shape: `TargetLinkReport`.

### `dalo target unlink <target>`

Disable a target in state. `unlink` itself removes nothing from the target
directory. The next `dalo sync` then reconciles the target and removes the
Dalo-owned links it had materialized there; unmanaged files and real
directories are never touched.

Examples:

```sh
dalo target unlink codex
dalo --json target unlink generic
```

JSON output shape: `TargetUnlinkReport`.

### `dalo source add <id> <git-url-or-path>`

Add a trusted team source, clone it into `sources/<id>/checkout`, configure it
with `update_policy = "track"`, and run a deterministic security preflight for
every discovered skill. Source IDs must be a single path component using only
letters, digits, `.`, `_`, and `-`; `.` and `..` are rejected.
Local paths are supported; relative paths resolve against the current working
directory.

Examples:

```sh
dalo source add platform git@github.com:example/platform-skills.git
dalo --dry-run source add team https://github.com/example/team-skills.git
```

JSON output shape: `SourceAddReport`.

### `dalo source add-catalog <id> <git-url-or-path>`

Add an untrusted catalog source, clone it into `sources/<id>/checkout`, and
configure it with `update_policy = "pin"`. Catalog skills are offers; nothing
from a catalog becomes active until selected and approved.
After adding a catalog, the CLI reports how many skills are available and prints
the exact `source inspect` and `source select` commands for the next step.
Local paths are supported; relative paths resolve against the current working
directory.

Examples:

```sh
dalo source add-catalog public https://github.com/example/skill-catalog.git
dalo source inspect public
dalo source select public review-helper
```

JSON output shape: `SourceConfig`.

### `dalo source list`

List configured sources in priority order. Git-backed entries include a
read-only `provenance` object assembled from config, `source-lock.toml`, and the
checkout without fetching: management authority, credential-redacted origin,
requested ref, resolved pin, and checkout commit.

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

Select catalog skills by stable frontmatter ID, `<source-id>:<slot>` reference,
or slot name. Selection runs the deterministic security preflight, writes
`config.toml`, and updates `source-lock.toml` with the pinned commit and
inventory snapshot. Human-readable output names the skills added or removed,
reports no-op requests explicitly, and shows the complete resulting selection
as secondary detail.

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
non-zero when selected skills drifted upstream. `--advance` explicitly updates
only this catalog after candidate approval, dependency, audit, and
materialization preflight. Selected removal, dirty or divergent checkouts,
security-audit blocks, and unsafe target state stop the transaction.

Examples:

```sh
dalo source refresh public
dalo source refresh public --check
dalo --json source refresh public
dalo --dry-run --json source refresh public --advance
dalo source refresh public --advance
```

JSON output shape: `CatalogDrift`.

With `--check`, the command exits with code 1 when a selected skill changed,
moved, or was removed upstream. New unselected offerings remain informational.

`--advance` and `--check` are mutually exclusive. Global `--dry-run` with
`--advance` returns `CatalogAdvanceReport`, including the complete old and new
catalog lock entries, stable drift codes, reconciled selection, security audit
results, materialization plan, and blocking reasons. It performs no config,
lock, checkout, audit-cache, state, or target writes. A real advance updates the
checkout, `source-lock.toml`, any stable-ID move in `config.toml`, `lock.toml`,
and affected owned target links as one rollback-safe transaction. Newly
required unapproved skills remain unlinked and can be approved after the new
pin is installed; their dependents remain blocked until then. A candidate with
blocking audit findings stays in `sources/.audit-staging/`; the report prints
the exact staged path to review and, when justified, accept for that content
hash before retrying the advance.

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
pending approvals, blocked required closure, blocking or failed security audits,
blocked materialization operations, missing targets for active skills, actionable
resolution diagnostics, lock drift, unmanaged blockers, instruction-block drift,
or unhealthy autosync state. It keeps the full report on stdout for JSON
consumers.

### `dalo sync`

Refresh clean tracking team sources, resolve the desired skill set, materialize owned symlinks into linked targets, and write `lock.toml`. Dirty tracking sources block refresh. Dalo does not overwrite unmanaged real directories or foreign symlinks.

Before materialization, Dalo audits the exact content hash of every active
skill. Unaccepted `high` or `critical` findings stop the command before any
links are changed. A previously recorded risk acceptance is reused only while
the complete directory hash, audit-engine versions, coverage, and finding set
remain unchanged.

Tracking team updates are fetched and audited in a detached worktree below
`sources/.audit-staging/` before the live checkout fast-forwards. Existing
target links therefore continue to expose the last accepted commit. When an
update is blocked, the error prints a command for auditing and accepting the
exact staged skill; the next `sync` reuses that staged hash.

Examples:

```sh
dalo sync
dalo sync --check
dalo --dry-run sync
dalo --json sync
```

JSON output shape: `SyncReport`.

`dalo --dry-run sync` does not fetch tracking team sources, so it prints a
note when upstream changes are not reflected in its plan. Run a real
`dalo sync` to fetch those sources first; JSON consumers can inspect
`SyncReport.unrefreshed_tracking_sources[]`.

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
preserved target link as if it were an unmanaged conflict. Machine-readable
consumers can inspect each affected source in `SyncReport.degraded_sources[]`.

Lock drift for local and team sources is commit-based. Uncommitted working-tree
edits do not change the recorded commit, and materialized symlinks expose those
live edits directly. Catalog selections are the source kind with content and
metadata fingerprints for upstream drift checks.

### `dalo autosync install|status|uninstall`

Install recurring `dalo sync --check` behavior through the current user's
native scheduler. macOS uses launchd. Linux prefers a systemd user timer and
falls back to cron only when the user manager is unavailable. Supported
schedules are `hourly`, `daily` (default for a first install), and `weekly`.
Omitting `--schedule` on a reinstall preserves the installed schedule.

Daily and weekly schedules run at a fixed per-store time in the local
early-morning window (00:00–05:59, derived from the store path so different
stores stagger rather than all firing at once). launchd coalesces missed jobs
on wake and the systemd timer uses `Persistent=true`, but the cron fallback has
no catch-up: a machine powered off throughout its scheduled window simply waits
for the next slot, so prefer `hourly` on machines that are usually asleep at
night. The window also overlaps the daylight-saving transition, so a skipped or
repeated local hour can shift a single run.

```sh
dalo autosync install
dalo autosync install --schedule hourly
dalo --json autosync status
dalo autosync uninstall
```

Installation records the stable absolute executable or launcher path that was
invoked, preserving Homebrew symlinks and the global npm launcher instead of
pinning their version-specific targets. Temporary `npx` executions cannot
install autosync; install `getdalo` globally or use another persistent Dalo
installation first. Generated artifacts never depend on shell startup files.
Reinstalling or removing the job is idempotent. Global `--dry-run` previews
install and uninstall without writing config, metadata, native artifacts, or
scheduler state. Re-run `autosync install` after moving the Dalo executable;
status treats a missing recorded executable as disabled instead of guessing a
replacement, and `doctor` reports the exact missing path.
If `autosync.toml` is malformed or uses a newer unsupported schema,
`autosync uninstall` quarantines it as `autosync.toml.corrupt-*`, reconstructs
the native scheduler identifiers from the store path for best-effort cleanup,
and leaves autosync ready for a clean reinstall.

The internal scheduled runner acquires the store lock once without waiting. If
an interactive Dalo process owns it, the run exits successfully as `skipped`
and retries on the next schedule. Dirty sources, malformed locks, pending
approvals, security findings, target conflicts, or managed instruction drift
remain fail-closed. `autosync-run.toml` records the last attempted and last
successful timestamps plus `running`, `succeeded`, `skipped`, or `blocked` and
an actionable reason. A run left `running` well past its schedule interval —
for example after a crash or power loss mid-sync — is surfaced by `status` and
`doctor` as an interrupted run.

`autosync status`, normal `status`, and `doctor` all surface this durable
state; non-JSON output renders timestamps as UTC calendar times and, when an
installed job is disabled, names the specific cause (missing artifacts, a moved
store, or a scheduler-reported disable). Logs are written to `autosync.log` and
`autosync-error.log` in the store; they are appended without rotation and are
removed on `autosync uninstall`.

### `dalo adopt <skill> [--replace]`

Copy an unmanaged target skill into `local/skills/<slot>`. With `--replace`, Dalo replaces the original unmanaged directory with an owned symlink after copying. Without `--replace`, the original directory remains untouched.

Adoption runs the same deterministic security preflight before copying or
replacing anything. `--reviewer` (with deprecated `--agent` alias), `--refresh-audit`, and `--accept-risk <reason>`
have the same meaning as on `dalo approve skill`.

The `<skill>` argument can be a slot name, a disambiguating path, or an ID reported by `status` or `resolve list`. If the slot name exists in more than one target, Dalo refuses the ambiguous selector and lists the paths to choose from.

Before it audits or copies anything, adoption rejects a folder name or
`SKILL.md` frontmatter `name` that is not a portable lowercase slot name.
Rename the folder or correct the frontmatter first, then run `dalo adopt`
again.

Examples:

```sh
dalo adopt review-helper
dalo adopt review-helper --replace
dalo --dry-run adopt /path/to/target/review-helper
```

When `--replace` reuses an existing local copy, Dalo compares the complete
directory shape before removing the unmanaged original: file bytes and
executable bits, empty directories, and symlink targets must all match.

Successful JSON output combines the preflight and mutation as
`{ "audit": AuditReport, "adoption": AdoptReport }`. If the audit blocks the
operation, Dalo prints only the blocking `AuditReport` and exits non-zero.

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

JSON output matches `dalo adopt`: successful output is
`{ "audit": AuditReport, "adoption": AdoptReport }`; a blocking audit prints
only `AuditReport` and exits non-zero.

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

Remove a recorded owned symlink by ID. If the recorded path is already missing,
Dalo drops the stale state record. If a different, foreign symlink occupies the
path, Dalo leaves that symlink untouched and drops only the stale ownership
record. If a real entry exists at that path, Dalo blocks removal.

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
Doctor emits `source_lock_ok` only when `source-lock.toml` exists and parses. If
the file is malformed, it reports `source_lock_invalid` and suppresses dependent
catalog provenance comparisons so recovery guidance cannot contradict itself.

### `dalo audit <skill-or-path>`

Inspect a skill before it is exposed to an agent. The target can be an existing
directory or a source-qualified reference. Deterministic checks are local and
never execute skill code.

```sh
dalo audit public:review-helper
dalo audit ./my-skill --check
dalo audit public:review-helper --reviewer auto
dalo --json audit public:review-helper --reviewer codex
dalo audit public:review-helper --reviewer claude --refresh-audit
```

Use `--refresh-audit` to ignore a compatible cached semantic review and run the
selected provider again. The older `--refresh` spelling remains a hidden alias
for script compatibility.

`--reviewer auto|codex|claude|opencode` adds a semantic review through an installed
agent CLI. Dalo starts a fresh non-persistent reviewer and treats a bounded
snapshot as untrusted data. Claude and OpenCode run with tools denied. Codex
currently retains its network-disabled, read-only sandbox shell, so Dalo never
selects it through `auto`; choosing `--reviewer codex` is an explicit acceptance of
that weaker isolation boundary. User configuration, project rules, skills,
plugins, and MCPs are disabled where the provider supports it. The structured
result contains evidence-backed findings, expected capabilities, and behavior
not disclosed by the skill. Depending on the installed agent configuration,
this can send skill contents to an external model provider and consume that
provider's quota. Dalo never includes `.git` metadata in that snapshot; a skill
containing a `.git` entry receives a blocking, partial-coverage finding instead.
Provider processes receive only an explicit runtime and provider-authentication
environment allowlist rather than inheriting Dalo's full environment. Omitting
`--reviewer` is fully local. `--agent` remains a deprecated compatibility alias.

Agent review is optional and additive, not an approval mechanism. It can add
evidence-backed findings to the deterministic audit, but it never clears a
deterministic finding or certifies a skill as safe. A review with no additional
findings means only that this constrained assessment found no additional issue;
it is not an endorsement or safety guarantee. `sync` does not invoke a provider
on its own. Its human-readable result repeats that boundary after every sync.

`--check` exits non-zero for unaccepted `high` or `critical` findings. Record a
reviewed exception with a non-empty reason:

```sh
dalo audit public:review-helper \
  --accept-risk "reviewed pinned upstream installer"
```

Persistence changes (such as shell startup files, scheduled tasks, or agent
configuration) and privileged command execution are `high` findings and block
by default. General dynamic execution remains a `medium` review finding: it is
reported, but does not block materialization on its own. This policy keeps
high-confidence persistence and privilege-escalation primitives behind an
explicit, content-bound risk acceptance without treating every technical skill
as unsafe by default.

Reports live below `audits/` and are keyed by both the source-qualified skill
reference and the complete skill directory hash. Cached agent results
additionally require the same provider and Dalo review-prompt version. Risk
acceptance is bound to source provenance, engine versions, coverage, and exact
deterministic and semantic findings, so a different source or newly discovered
risk requires a new decision even when the skill bytes did not change.

The audit directory and report files are restricted to the current user when
Dalo writes them. Audit state is still a local trust boundary rather than a
cryptographically authenticated log: a process running as the same user can
replace it. Protect the store like other security-sensitive user state and do
not place it in a shared or untrusted writable directory. No-finding results
are best-effort observations, not a security certification.

JSON output shape: `AuditReport`.

### `dalo approve`

Grant, list, and revoke approval records without editing `approvals.toml`.
Every skill, agent, author, and organization value is source-qualified, so an approval
cannot accidentally apply to a different source with the same name.

```sh
dalo approve list
dalo approve skill public:review-helper
dalo approve skill public:review-helper --reviewer codex
dalo approve skill public:review-helper --accept-risk "reviewed exception"
dalo approve agent team:reviewer
dalo approve source team
dalo approve author public:maintainers
dalo approve org public:example-org
dalo approve revoke skill public:review-helper
```

Approval writes support `--dry-run` and `--json`. A pending skill shown by
`status` can be approved narrowly with `dalo approve skill <source:skill>`.
A pending canonical agent shown by `agent list` can be activated with
`dalo approve agent <source:agent>`. The revoke scope is one of `skill`,
`agent`, `source`, `author`, or `org`; Clap validates
this value and exposes the choices to shell completion.
Skill approval always runs the deterministic preflight first and refuses a
blocking result unless a reason is supplied with `--accept-risk`. `--reviewer`
adds the same isolated semantic review as `dalo audit`.

JSON output shapes: `ApprovalsFile` for `list`; successful `approve skill`
output is `{ "audit": AuditReport, "approval": ApprovalReport }`; `agent`,
`source`, `author`, `org`, and `revoke` mutations emit a bare `ApprovalReport`. If the
skill audit blocks approval, Dalo prints only the blocking `AuditReport` and
exits non-zero.

### `dalo instructions enable <pack> <file>`

Render a local instruction pack from `local/instructions/<pack>.md` into an instruction file as a managed block. The target file is created if missing. Enabling the same pack again is idempotent and refreshes the block.

Examples:

```sh
dalo instructions enable team-style ~/.codex/AGENTS.md
dalo --dry-run instructions enable team-style ./AGENTS.md
```

JSON output shape: `InstructionPackReport` with `pack_id`, `target`, `action`,
`dry_run`, and an optional `warning` when recovery leaves a malformed target
block untouched.

### `dalo instructions disable <pack> <file>`

Remove a pack's managed block from an instruction file and remove its active lock entry.

Examples:

```sh
dalo instructions disable team-style ~/.codex/AGENTS.md
dalo --json instructions disable team-style ./AGENTS.md
```

JSON output shape: `InstructionPackReport` with `pack_id`, `target`, `action`,
`dry_run`, and an optional `warning` when recovery leaves a malformed target
block untouched. Disabling a pack removes its lock entry even when its managed
markers are malformed; the target file is left unchanged and the warning
explains the remaining drift. Mutations abort if the target changed on disk
after it was read. Target updates lock the opened inode and verify both its
content and identity before and after writing, so a file replaced by another
process is left untouched and the command fails safely.

### `dalo instructions list`

List active instruction packs recorded in `lock.toml`.

Examples:

```sh
dalo instructions list
dalo --json instructions list
```

JSON output shape: `InstructionPackListReport` with
`active_instruction_packs[]` entries.

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
| `1` | expected failure | User-actionable input/state problem, such as semantic value validation, unknown source, unknown target, unsupported schema, parse error, a failed explicit check, a security-audit block during `sync` or `approve`, or adoption destination already existing. |
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
| `team init` | `TeamManifestMutationReport` | `path`, `action`, `dry_run`, resulting `manifest` |
| `team catalog add` | `TeamManifestMutationReport` | `path`, `action`, `catalog_id`, `dry_run`, resulting `manifest` |
| `team catalog skills` | `TeamManifestMutationReport` | `path`, `action`, `catalog_id`, `dry_run`, resulting `manifest` |
| `team catalog version` | `TeamManifestMutationReport` | `path`, `action`, `catalog_id`, `dry_run`, resulting `manifest` |
| `team catalog update` | `TeamCatalogUpdateReport` | `catalog_id`, `old_version`, `old_commit`, `from_ref`, `candidate_commit`, `outcomes[]`, `audits[]`, optional `accepted_risk_reason`, `blocking_reasons[]`, `dry_run`, `updated`, resulting `manifest` |
| `team catalog remove` | `TeamManifestMutationReport` | `path`, `action`, `catalog_id`, `dry_run`, resulting `manifest` |
| `team show` | `TeamManifestView` | `path`, `manifest` |
| `source add` | `SourceAddReport` | `source`, `dry_run`, `audits[]` with one `AuditReport` per discovered skill |
| `source add-catalog` | `SourceConfig` | `id`, `kind`, `path`, `priority`, `enabled`, `trusted`, `url`, `update_policy`, `selection` |
| `source list` | `SourceListReport` | `sources[]`, each with existing `SourceConfig` fields plus `provenance` |
| `source priority` | `SourcePriorityReport` | `source`, `dry_run` |
| `source inspect` | `CatalogInspectReport` | `source_id`, `candidates[]` |
| `source select` | `CatalogSelectReport` | `source_id`, changed user references in `added[]` / `removed[]`, complete resulting `selected[]`, `dry_run`, `audits[]` for skills named by the operation, `migration_warnings[]` for degraded legacy sibling catalogs |
| `source refresh` | `CatalogDrift` | `source_id`, `pinned_commit`, `upstream_commit`, `outcomes[]`, `migration_warnings[]` for degraded legacy sibling catalogs |
| `source refresh --advance` | `CatalogAdvanceReport` | exact `old_lock`/`new_lock`, selections, `outcomes[]`, `audits[]`, `sync`, `blocking_reasons[]`, `dry_run`, and `advanced` |
| `source remove` | `SourceRemoveReport` | `source_id`, `checkout_path`, `kept_checkout`, `removed_approvals`, `removed_catalog_lock`, `reconciled_links[]`, `deactivated_skills[]`, `cleanup_warnings[]`, `affected_paths[]`, `dry_run` |
| `autosync install` / `uninstall` | `AutosyncMutationReport` | `action`, `dry_run`, resulting `status` |
| `autosync status` | `AutosyncStatusReport` | `configured`, `installed`, `enabled`, backend, schedule, executable, store, artifacts, optional `scheduler_error`, optional `disabled_reason`, and optional `last_run` |
| `status` | `StatusReport` | `store`, `sources[]` with `provenance`, `targets[]`, `inventory_warnings[]`, `resolution`, dry-run `materialization[]`, `blocking_audits[]`, `audit_failures[]`, `lock`, `unmanaged_skills[]`, `target_warnings[]`, `instruction_packs[]`, `instruction_pack_overlaps[]`, `instruction_block_drifts[]`, `autosync` |
| `sync` | `SyncReport` | `store`, `dry_run`, `linked_targets`, `operations[]`, `resolution`, `degraded_sources[]` (`id`, `path`, `reason`), `unselected_catalogs[]` (`source_id`, `available_skills`) |
| `audit` | `AuditReport` | `schema_version`, `source_ref`, `skill_path`, `content_hash`, `static_engine_version`, `scanned_at_unix`, `coverage`, `status`, optional `max_severity`, `static_findings[]`, optional `agent_review`, optional `risk_acceptance` |
| `approve list` | `ApprovalsFile` | `schema_version`, `approvals[]` |
| `approve skill` | audited approval outcome | `audit` (`AuditReport`), `approval` (`ApprovalReport`) |
| `approve agent` / `source` / `author` / `org` / `revoke` | `ApprovalReport` | `scope`, `value`, `action`, `dry_run` |
| `adopt` / `resolve adopt` | audited adoption outcome | `audit` (`AuditReport`), `adoption` (`AdoptReport`) |
| `resolve list` | `ResolveListReport` | `unmanaged_skills[]`, `target_warnings[]`, `owned_skills[]` |
| `resolve keep` | `KeepReport` | `skill`, `existing`, `dry_run` |
| `resolve unkeep` | `UnkeepReport` | `selector`, `removed[]`, `dry_run` |
| `resolve remove-owned` | `RemoveOwnedReport` | `id`, `link_path`, `status` |
| `doctor` | `DoctorReport` | `store`, `findings[]`, `summary` |
| `instructions enable` / `disable` | `InstructionPackReport` | `pack_id`, `target`, `action`, `dry_run`, optional `warning` |
| `instructions list` | `InstructionPackListReport` | `active_instruction_packs[]` with `pack_id`, `target`, `source_id`, optional `commit`, optional `version` |

Each `AuditReport.static_findings[]` entry contains `id`, `severity`,
`category`, `path`, optional `line`, `message`, and optional bounded `evidence`.
When present, `risk_acceptance` contains `reason`, `accepted_at_unix`, and
`scope_hash`; `agent_review` identifies the provider and isolation boundary and
includes its findings, summary, expected capabilities/actions, and undeclared
behaviors. `source add` and `source select` expose security preflight results in
their top-level `audits[]` arrays. `source add-catalog` returns only the new
`SourceConfig`: adding a pinned catalog does not audit anything until a skill is
selected.

Each `StatusReport.audit_failures[]` entry contains `source_ref`, `source_id`,
and the technical `reason`. The failed skill is omitted from the active
materialization plan, while the owning source is treated as degraded so an
existing owned link is not removed solely because the audit was incomplete.

Common status values:

| Field | Values |
| --- | --- |
| `TargetSupport` | `supported`, `experimental` |
| `TargetLinkStatus` | `planned`, `linked`, `updated`, `existing` |
| `TargetUnlinkStatus` | `planned`, `unlinked`, `missing` |
| `MaterializeOperationKind` | `create`, `relink`, `remove`, `drop_record`, `conflict`, `keep`, `no_op` |
| `MaterializeOperationStatus` | `planned`, `applied`, `existing`, `blocked` |
| `AdoptCopyStatus` | `planned`, `copied`, `existing` |
| `AdoptReplacementStatus` | `planned`, `replaced`, `skipped`, `protected` |
| `RemoveOwnedStatus` | `planned`, `removed`, `dropped_missing`, `blocked_real_entry`, `dropped_foreign_symlink` |
| `InstructionBlockDriftKind` | `missing`, `malformed`, `stale`, `source_missing` |
| `CatalogDrift.code` | `new_available`, `selected_changed`, `selected_moved`, `selected_removed` |
| `AutosyncRunOutcome` | `running`, `succeeded`, `skipped`, `blocked` |
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

`SourceProvenance` contains `management` (`direct` or `team_manifest`), optional
`declared_by`, credential-redacted `origin_url`, optional `requested_ref`, the
canonical `resolved_commit`, and the observed `checkout_commit`. For catalogs,
the resolved commit comes from `source-lock.toml`; a differing checkout commit
is preserved in output so drift remains visible.

Resolution diagnostics use these codes when present in `status.resolution.diagnostics`: `pending_approval`, `local_override`, `shadowed`, `required_expanded`, `cross_source_require`, `required_blocked`, `legacy_bare_approval`, and `audit_failed`. For recovery steps, see [Troubleshooting and FAQ](troubleshooting.md).

## Store Layout

After `dalo init`, the store contains:

| Path | Purpose |
| --- | --- |
| `config.toml` | User-authored config: settings and source list. |
| `lock.toml` | Resolved user lock written by `sync` and instruction commands. |
| `state.toml` | Internal target/materialization/protection state. |
| `approvals.toml` | Local approval records. |
| `source-lock.toml` | Catalog source pins, selections, and inventory snapshots. |
| `autosync.toml` | Installed scheduler backend, schedule, exact paths, identifier, and artifacts. |
| `autosync-run.toml` | Last attempted/successful scheduled run and its durable outcome/reason. |
| `autosync.log`, `autosync-error.log` | Native scheduler stdout and stderr. |
| `audits/<content-hash>-<source-ref-hash>.json` | Source- and content-bound deterministic and optional agent security reports. |
| `.lock` | Temporary coarse lock file while mutating commands run. |
| `local/skills/` | Local private skill directories. |
| `local/instructions/` | Local instruction pack Markdown files. |
| `sources/<id>/checkout/` | Team and catalog Git checkouts. |
| `sources/.audit-staging/` | Detached incoming team commits retained only while security review is required. |

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
| `settings.autosync` | Whether `dalo autosync install` has configured a scheduler job. |
| `settings.sync_interval` | Installed `hourly`, `daily`, or `weekly` schedule. |
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
| `sources[].declared_by` | Team source whose `dalo.toml` owns this derived catalog. Generated by `sync`; do not edit directly. |
| `sources[].declared_ref` | Git version requested by the declaring team manifest. Generated by `sync`. |

Unknown fields are rejected.

## Team repository `dalo.toml`

Schema version: `schema_version = 1`.

Team repositories may compose their checked-in skills with pinned external
catalogs. `sync` namespaces ordinary IDs as `<team-id>.<catalog-id>` and stores
the resolved commit in `source-lock.toml`. If either component itself contains
a dot, Dalo losslessly encodes both components into a reserved point-free
`team-<hex>-catalog-<hex>` ID so different declarations cannot collapse onto
the same source.

```toml
schema_version = 1

[source]
id = "company"
name = "Company Skills"
kind = "team"

[[catalog]]
id = "marketing"
url = "https://github.com/coreyhaines31/marketingskills.git"
version = "0123456789abcdef0123456789abcdef01234567"
skills = ["+copywriting", "+launch", "+seo-audit", "-seo-audit"]
priority = 11
```

Catalog fields:

| Field | Meaning |
| --- | --- |
| `id` | ID local to the manifest. The local source ID is namespaced with the team source. |
| `url` | Git URL. Embedded credentials are rejected. |
| `version` | Required Git commit, tag, or ref. `ref` is accepted as an alias. Immutable commits are recommended. |
| `skills` | Include/exclude filters. Omitted or empty means all. |
| `priority` | Optional global resolver priority; defaults to the team source priority plus one. |

Filter evaluation is set-based and independent of list order:

- no filters: all catalog skills
- only `-skill`: all except the exclusions
- any `+skill` or bare `skill`: whitelist mode
- exclusions always win over inclusions

Unknown or ambiguous filter references block sync. Manifest-derived catalogs
are untrusted by default, so their newly selected skills remain pending until a
local approval matches. `source select`, `source priority`, `source remove`, and
pin advancement reject derived catalogs; edit and review `dalo.toml` instead.
Changing a catalog URL uses an explicit two-sync replacement: remove the
declaration and sync, then add the reviewed replacement URL and sync again.

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
| `agent` | `<source-id>:<name>` or `<source-id>:<stable-id>` | One canonical agent package from one source. |
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

`SKILL.md` may reference another file inside the same source checkout, but a
metadata symlink whose resolved target escapes the checkout is skipped and
reported as `skipped_symlink`. This keeps skill identity and approval metadata
contained within the source being scanned.

## Instruction Packs

Instruction packs are Markdown files in `local/instructions/<id>.md` or
`<source>/instructions/<id>.md`. Dalo discovers both locations, but
`instructions enable` currently reads only local packs; source packs are
discovery-only. Pack IDs use the same safe token rule as source IDs: letters,
digits, `.`, `_`, and `-`, excluding `.` and `..`.

Dalo reads an optional `version:` entry from the first five lines and optional
`topics:` or `tags:` metadata from the first eight lines:

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

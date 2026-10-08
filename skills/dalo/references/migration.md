# Bring existing skills under Dalo

Start with the inventory and preserve the user's intended scope. Migration can
be partial: uncertain content can remain where it is while known items move.
Distinguish two outcomes before acting: preserving today's installed content as
a local snapshot, or reconnecting selected skills to a verified upstream source
for future updates. A local snapshot does not receive upstream updates.

## Suggest migration from the inventory

During a bare invocation, setup, or general assessment, a project
`skills-lock.json` should prompt a short recommendation without requiring the
user to ask about migration. For example, in the user's language: "This project
already has skills.sh installation records. I can check whether its installed
skills can move to Dalo without changing their content. Would you like me to
check?" Mention that verification reads Git sources over the network; the
preview leaves project files unchanged. Do not claim the migration is ready
before verification.

Check the installed CLI's help for `migrate skills-sh`. If it is unavailable,
explain that a Dalo update is needed for the native importer; do not run a
nonexistent command. If `dalo-project.toml`, `.dalo`, or
`.dalo-migration-backup` already exists, explain the mixed or interrupted setup
and propose inspecting it instead of offering an importer that will refuse it.
A global skills lock alone does not qualify for project migration. Unknown or
malformed project locks need diagnosis, not a promise of automatic conversion.

Offer once per conversation unless the installation changes or the user asks
again. Respect a decline or deferral. For unrelated narrow maintenance, finish
the requested work without adding a migration detour. An existing migration
request already authorizes verification; do not repeat the question. Agreement
to a preview authorizes verification only: show the actual result, blockers,
agent targets, backup location, and install/approval steps before offering the
handover. Existing authorization for that handover remains valid.

## Project skills.sh installations

Once verification is requested and the CLI supports it, use the native importer:

```sh
dalo --project "$project" --json migrate skills-sh
```

Review all blockers and inferred targets. With migration authorized, apply using
`dalo --project "$project" migrate skills-sh --apply`, then run `dalo install` in
that project, review and approve pending skills locally, and install again. The
generated definition uses local approval; switching it to
`approval = "declaration"` is a separate declaration change for the team's review.
The importer verifies complete installed content against the recorded Git ref
(or current default branch); it never treats `computedHash` as a commit. A
changed source or local edit blocks the entire apply. Preserve such content and
resolve its intended ownership rather than bypassing verification. This native
import is all-or-nothing; the partial manual migration guidance below does not
allow ignoring a blocked entry in its plan.

Applying backs up the old directories, aliases, and lock under
`.dalo-migration-backup/` before writing `dalo-project.toml`. Keep that backup
until installation succeeds. It does not import approvals; skills are temporarily
inactive until installation finishes. It supports version 1 project locks with
Git origins and Dalo-supported project folders, not global locks or all foreign
agent/subagent formats. See the project migration documentation for recovery.

## Choose the destination from evidence

| Existing content | Default recommendation |
| --- | --- |
| User-authored, modified, or unknown real directory | Preserve its full contents in the local source, or intentionally keep it unmanaged |
| Verified unmodified third-party install with a Git origin | Reconnect through a catalog, selecting only the intended skills |
| Verified team repository the user trusts | Reuse or add the scoped team source |
| Foreign symlink | Inspect its canonical content and all consumers before choosing a handover |
| Project-only skill | Keep project scope; explain any target/store limitation before changing scope |
| Same name with different content | Preserve both; obtain the intended precedence or naming choice |

When provenance or the original revision cannot be recovered, keep the installed
content. Do not silently substitute the current upstream version. HTTP-only,
package-based, or other non-Git origins are not automatically Git sources.

## Resolve name and target conflicts

First classify the conflict from the machine-readable reports:

```sh
dalo --store "$store" --json status
dalo --store "$store" --json resolve list
```

`status` shows active and shadowed managed skills, target materialization
blocks, and unmanaged entries. `resolve list` gives exact unmanaged IDs and
paths, scan warnings, and recorded Dalo-owned links. Match the source-qualified
skill and target path before acting; when a selector is ambiguous, use the ID
or path reported by the CLI. A `sync` preview covers the entire store and every
linked target, so show all affected skills and agents before applying it.

- **Two managed sources offer the same slot:** the lower numeric source priority
  wins. If the user wants a different winner, preview the exact source priority
  change with `--dry-run`. After the user authorizes that exact policy change,
  apply it, then preview sync under the new priority. A policy dry-run does not
  update config, so a later sync dry-run still uses the old priority. If the
  user wants a wholly read-only combined preview, explain that separate
  dry-runs do not compose.
  Check `source list --json` before offering either policy command. Local source
  priority is fixed and local skills cannot be namespaced. If `declared_by`
  names a team source, its catalog priority and namespace are controlled by
  that team's `dalo.toml`; use the existing team catalog workflow in
  [Maintenance](maintenance.md#updates) rather than editing consumer config.
  If both should coexist, a namespace is an option only when the user accepts
  changing every skill name from that source to `<prefix>__<skill>`. Preview the
  exact namespace change. After the user authorizes it, apply that change, then
  preview the whole-store sync under the new namespace before target mutations.
  As with priority, a namespace dry-run does not change config, and separate
  dry-runs do not compose into a read-only combined preview. A namespace
  affects materialized names; it does not rename source folders, edit
  references, or change source-qualified approvals. Namespaced team skills
  still follow normal upstream refresh and sync behavior. The built-in local
  source cannot be namespaced.
- **A real unmanaged directory occupies the requested target slot:** offer
  audited `resolve adopt <id>` to copy it into the local source while leaving
  the original in place, or `resolve adopt <id> --replace` when the user wants
  that exact directory replaced by a Dalo-owned link after the copy. Adoption
  creates a local snapshot; it does not keep receiving updates from the former
  installer or upstream. Without `--replace`, the original remains and can keep
  blocking that target slot. Adoption dry-run previews only the adoption; it
  does not stage a local skill for a following sync dry-run. Applying adoption
  can make the local skill win over a same-named managed source across other
  linked targets. Review the exact adoption audit and its copy/replacement
  effects, apply only the choice the user authorized, then inspect the new
  `status --json` and whole-store sync preview before any further target
  mutations. If the user wants to retain the directory unmanaged, use
  `resolve keep <id>`; the managed skill remains unavailable at that occupied
  target slot. Adopt only the entry and scope covered by the user's request.
- **A foreign symlink occupies the slot, or ownership is unclear:** preserve it
  and stop. `adopt`, `resolve keep`, and `resolve remove-owned` do not take over
  or remove a foreign symlink. Do not unlink or replace it as if it were a real
  directory. Follow the bounded foreign-entry handover only when that exact
  handover is requested; otherwise report the path and blocker.

Do not use a source-wide priority or namespace change to imply a per-skill
rename. A dry-run adoption likewise cannot be composed with a sync dry-run to
preview how the not-yet-created local skill would resolve. If neither supported
choice matches the request, preserve both versions and explain that Dalo has no
per-skill rename/adapt command.

## Adopt real directories

Dalo discovers adoptable directories only inside already configured targets.
Use an exact selector from `resolve list`, preferably the path when names repeat:

```sh
dalo --store "$store" --dry-run --json adopt "$skill_path"
dalo --store "$store" --json adopt "$skill_path"
dalo --store "$store" --dry-run --json adopt "$skill_path" --replace
dalo --store "$store" --json adopt "$skill_path" --replace
```

The first write copies into the local source and leaves the original in place.
The second replaces that original with a Dalo-owned link only when the existing
local copy still matches. Use the replacement when management of this exact
directory is within the migration request; a request merely to inspect or back
up a skill is not such a request. Adoption does not make a Git commit.

If the destination already exists with different content, preserve both and
resolve that choice. Do not delete either copy to make adoption pass. Invalid
slot names or frontmatter need a deliberate rename/edit, not a guessed mass
normalization. Use `resolve keep` for a real unmanaged directory the user wants
to retain. A copied-but-unreplaced skill can still conflict; report that state.

## Reconnect an upstream install

Use installer records as candidate provenance, then verify the repository,
skill path, and complete installed content. Inspect the candidate source and
compare before removing or replacing any existing entry. Adding a catalog pins
what Dalo resolves at that time; it does not import a skills CLI lock or restore
an arbitrary old ref. If the installed revision is unavailable or differs,
offer preservation as a local snapshot or an explicit update with a reviewed
diff. Do not copy foreign hash values into Dalo's lockfiles.

Reuse or add the catalog, select the exact skills, review their content and
audit results, and grant only the approval covered by the installation request.
Keep modified versions local, with an explicit precedence decision if a catalog
offers the same name. Do not both adopt and select an upstream copy without
explaining which version would win. Preview synchronization before handover.

## Handover of foreign entries

`adopt` and `resolve keep` do not handle foreign symlinks. `sync` will not take
them over, and `resolve remove-owned` is not a foreign-link removal command.

When the user requested full migration of specific foreign entries, prepare a
bounded, reversible handover using filesystem tools:

1. Record every affected path, raw link destination, canonical content location,
   and other discovered consumers. Check for Dalo ownership drift first. Stop
   that item if ownership or shared consumers are ambiguous.
2. Preserve the complete content in a unique recovery directory outside agent
   discovery folders and Dalo-managed checkouts. Record original paths and
   installer metadata there, preserving file modes and links. Verify the copy;
   retain both the original location information and recoverable content. A
   backup of only a symlink is not a backup of its content.
3. Prepare the selected Dalo source and inspect its audit and synchronization
   plan. For a local snapshot that cannot be adopted directly, copy the verified
   content into a new, unoccupied local skill directory; do not add ownership
   records yourself. If internal links would break after relocation, resolve
   that before switching. Do not overwrite an existing local skill.
4. Recheck the original entries and prepared content immediately before switching.
   If they changed, reassess. Move only the exact agreed unmanaged entries into
   the recovery directory without following them; never use a wildcard or remove
   a whole skill root. Shared canonical directories and dependent links must be
   treated as one handover so no remaining consumer is stranded.
5. Run Dalo synchronization, inspect each new link and its resources, and check
   every known shared consumer. If the intended links cannot be established,
   remove only newly created Dalo-owned links through Dalo and restore the saved
   entries into vacant original paths. If restoration would overwrite concurrent
   content, stop that item and report the exact recovery paths.

For a real directory being switched to a catalog, the same recovery process
applies. Prefer Dalo's adoption transaction when the intended destination is
the local source. Do not perform a manual handover during a bare invocation.

## Leave a usable outcome

Foreign installer records do not confer Dalo trust and do not transfer ownership.
Do not rewrite their schemas or automatically run `skills remove`/`skills update`
against paths now managed by Dalo. Report the old records and remaining external
installs; any cleanup must be scoped to those exact entries and preserve other
skills. Until reconciled, the old installer may still try to update migrated
paths. Keep recovery copies and explain which manager should be used for each
set. Include the assistant itself if it was bootstrapped through another manager;
do not strand or erase the skill currently guiding the operation.

Verify all migrated entries and report any that stayed external, conflicted,
or could only be preserved as snapshots. Never describe a partial migration as
complete solely because `sync` exited successfully.

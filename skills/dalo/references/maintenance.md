# Maintain an existing Dalo setup

Use the inventory reports to distinguish a binary update, upstream source
changes, pending approvals, ownership drift, and an old persisted schema. They
need different actions; “outdated” does not mean reinstall everything.

## Updates

Choose the update operation from the source's management authority and update
policy, not merely its Git URL. Tracking team sources follow their upstream;
personal catalog pins move only in an explicitly requested update. A catalog
declared by a team follows the team's declared pin, not that catalog's latest
upstream commit. Project installation restores the committed project definition;
updating that definition is a separate authoring operation.

- **Binary:** identify the executable and installation channel, then follow
  [setup](setup.md). The absence of an update notice in JSON or offline mode does
  not mean the installed version is current.
- **Tracking team source:** `sync` fetches clean tracking sources before
  resolving and materializing. `--dry-run sync` does not fetch them; inspect
  `unrefreshed_tracking_sources` and explain that the preview omits upstream
  changes. A real sync can apply unrelated safe work while another source stays
  degraded. Read the report and verify each requested source.
- **User-managed catalog:** `source refresh <id>` fetches and reports drift but
  does not advance the pin. Add `--check` when an automation needs selected-skill
  drift to produce a nonzero exit status. For an actual update, if supported,
  inspect `--dry-run --json source refresh <id> --advance`, review changed
  selections and blocking findings, then run `source refresh <id> --advance`.
  This can update affected target links directly. Newly required skills may
  still need approval.
- **Team-manifest catalog:** respect the authority reported by `source list`.
  The pin belongs to the team's `dalo.toml`. A consumer should refresh its team
  source; do not override the pin locally. If the task is to author the team's
  update, use the installed `team catalog update` help and the team's repository
  workflow. Do not commit, push, or publish a team update merely to refresh one
  consumer.
- **Project source:** after pulling a changed `dalo-project.toml`, run
  `dalo --project <root> --json install` to restore its declared pins. To author
  an update, preview `dalo --project <root> --json project update <id> --ref <ref>`.
  Apply a moving ref with the previewed `--expect-commit <commit> --apply`, then
  install; changed content remains subject to local approval. Repeat `--skill`
  only when replacing the selection is part of the request.

For team catalog authoring, preview
`dalo --dry-run --json team --repo <team-root> catalog update <id> --from <ref>`.
Use the same operation without `--dry-run` to edit the local manifest. Add `--pr`
only for a requested GitHub review proposal; commit and push are part of that
operation. An exact full commit can be supplied to `--from` when the reviewed
candidate must remain fixed. Consumers pick up the published declaration through
their normal team sync.

For “check for updates”, report differences without advancing pins or syncing
targets. Upstream checks fetch data; do not start them during a purely local
inventory or when offline. Do not implement a guessed update loop over every
source kind. A request to update all existing skills does not authorize new
source-wide approvals, new hook execution, or accepting audit exceptions.

The current CLI has no read-only upstream preview for a tracking team source:
`sync --dry-run` uses the existing checkout, and a real `sync` fetches and applies
eligible updates. `source refresh` is catalog-only. Personal catalog `--advance`
fetches its upstream again when applied and has no `--ref` or `--expect-commit`
option; a previous preview does not bind the subsequent candidate. Report these
limits when the task requires review of a fixed revision before application;
do not substitute raw Git pulls or edits to Dalo's state files.

## Agent-run updates

Use the agent or host's automation facility when the user requests recurring
updates. Keep the chosen scope, update policy, cadence, and notification
preferences in that automation. An update request alone does not choose a
schedule or enable Dalo autosync. Existing authorization for the named recurring
operation applies to later runs; new trust decisions still need their own review.

For an initialized global or explicit store, the ordinary tracking update is:

```sh
dalo --store "$store" --json sync --check
```

`--check` validates the resulting sync; it still mutates sources and targets.
It is not an upstream-check-only flag. Personal catalog pins stay unchanged.
For a project, use `dalo --project <root> --json install` to restore the current
definition; project scope does not support `sync` or `autosync`.

Read the JSON report even when the command fails. Runtime errors may be JSON on
stderr; usage errors remain plain text. Inspect changed sources, pending
approvals, degraded sources, and blocked operations rather than treating an exit
code alone as a complete result. Stop for a safety or approval block instead of
granting trust, accepting audit findings, or discarding local edits in a retry.
Use `status --check --json` and `doctor --check --json` for the same explicit scope
when diagnosis is needed. Leave retry timing and notifications to the automation;
do not report a successful upstream check when fetching failed or was skipped.

## Share a local skill

When the user wants to contribute a local skill to a team or community
repository, use Dalo's review-first promotion flow. First check `dalo promote
--help` and the configured target with the installed binary; promote currently
supports directly configured GitHub.com team repositories.

```sh
dalo --dry-run promote <skill> --target <team>
dalo promote <skill> --target <team>
```

The preview is local and does not fetch or write. Read its selected skill,
destination, and static audit result. Promotion itself creates a branch, commit,
pushes it, and opens a PR, so run it only when the user has requested that
contribution. For a destination without direct push access, explain that `--fork`
creates or reuses a fork under the authenticated GitHub account; use it when that
is within the requested destination. Never push to the default branch. If the
user specifically asks to contribute edits from the selected team checkout, use
`--from-dirty`; it updates that skill in the PR when the checkout matches the
current base, and blocks unrelated checkout edits. A blocking audit needs a
separately reviewed, content-bound
acceptance before retrying. Do not guess an acceptance reason.

Dalo copies only the selected skill into a temporary repository clone; with
`--from-dirty`, it replaces only that slot in the temporary PR branch. It does
not change the source skill or the existing team checkout. Read the returned PR
URL and mention the source commit and deterministic audit summary. If pushing
succeeds but PR creation fails, preserve the remote branch and report it for
recovery rather than retrying into a new branch automatically.

## Keep edited instruction blocks

When the user wants to keep edits inside an active team instruction block, check
`instructions adopt --help` with the installed binary and preview the specific
pack and file:

```sh
dalo --dry-run instructions adopt <source:pack> <file>
dalo instructions adopt <source:pack> <file>
```

Inspect the body and local destination in the preview. Adoption replaces only
that file's active source block with a local pack; other targets remain on the
team pack. Existing local packs, malformed markers and dirty or changed source
commits block adoption. Preserve the user's content and diagnose the blocker.
The local pack is left uncommitted for review. Use its local pack ID for later
enable/disable operations; do not re-enable the team pack on top of the override
unless the user wants both.

This applies to rendered instruction copies. Editing a symlinked skill changes
its underlying source immediately, and agents can see the edits before any
sync. Use the existing skill migration/local variant workflow for those edits;
a future dirty-source check does not isolate agent access.

## Repair and resume

| Finding | Response |
| --- | --- |
| Dirty team checkout | Inspect and preserve the diff; do not reset, clean, stash, or commit the user's work automatically |
| Missing or degraded source | Restore access or diagnose the checkout; preserve existing owned links |
| Repointed link or real content at an owned path | Inspect that content and ownership; do not treat it as an ordinary adoptable skill |
| Pending skill approval | Inspect the exact source-qualified skill and audit; approve only within the requested trust scope |
| Blocking security finding | Explain the finding and staged content; do not invent an `--accept-risk` reason |
| Catalog selection removed upstream | Preserve the current pin until the user chooses a replacement or removal |
| Unmanaged conflict | Classify it with `status --json` and `resolve list --json`, then follow [the conflict playbook](migration.md#resolve-name-and-target-conflicts) for the exact entry |
| Informational `schema_migration_pending` | Let a supported ordinary write migrate that file; do not rewrite version numbers |
| Malformed or unsupported store schema | Preserve the files and diagnose compatibility; do not delete or initialize over the store |

Before a binary/store upgrade, preserve a recoverable copy of store state and
local work and inspect release-specific compatibility guidance. A lazy schema
migration may remain pending after sync because only another command writes
that file. Do not force unrelated writes merely to clear informational findings.
Use the [upgrade guide](https://dalo.sh/docs/upgrading.html) and
[diagnostic reference](https://dalo.sh/docs/troubleshooting.html) for the installed
and destination versions; documentation for a newer release is not proof that
an older binary supports its flags.

After upgrading the binary, check `assistant install --help`. For an existing
bundled assistant, preview and run `assistant install` to match the skill to the
installed release. Existing target symlinks read that update immediately.
Preserve a blocked or customized bundle; do not delete its receipt to force an
update. If the skill is owned by another installer or Dalo source, keep using
that source's update route instead of creating a competing bundled copy.

## Everyday changes

For an additional global skill, reuse the appropriate source, inspect available
selectors, and select/review/approve only the named item. When the user wants a
repository skill shared through Git, use the project's `dalo-project.toml` and
`dalo project add` workflow in [setup](setup.md): preview the exact source,
selection, and commit; apply a moving ref only with the previewed
`--expect-commit`; then run `dalo install` and handle local approvals. To move an
existing project source forward, use `dalo project update` to review the new
pin, inventory, dependency changes, and audits before applying it. Installation
revokes per-skill approvals for changed or removed content, including required
dependencies and previously deselected skills. Review and approve the changed
content locally before retrying installation. Do not add a project dependency to
the global store. Connect another agent through setup and preview the resulting
active set before syncing.

For project removal, preview `dalo project remove <source-id>` or repeat
`--skill <exact-declaration-selector>` for individual selections. Show dependency,
approval, and link effects before applying. Apply changes only to the declaration,
then run `dalo install`; teammates use the same command after pulling it. Remaining
consumers keep required skills active. Removing the final selector removes the
source. Cached checkouts, including dirty content, remain intact; do not delete
`.dalo`, reset approvals, or remove caches to make the declaration converge.
Foreign target entries are preserved. Dirty sources that remain declared still
block install; help preserve those edits instead of overwriting them.

For global removal, distinguish a catalog selection (`source unselect`), an entire
source (`source remove`, preview first), and a target (`target unlink`, followed
by sync to remove its owned links). Unlinking a target alone removes no files.
`resolve remove-owned` repairs a recorded link; an active skill can reappear on
the next sync. Do not present that repair as permanent uninstallation or delete
local skill content to hide it from one agent. Report when per-target filtering
or another requested behavior is unsupported instead of editing internal state.

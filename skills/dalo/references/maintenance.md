# Maintain an existing Dalo setup

Use the inventory reports to distinguish a binary update, upstream source
changes, pending approvals, ownership drift, and an old persisted schema. They
need different actions; “outdated” does not mean reinstall everything.

## Updates

- **Binary:** identify the executable and installation channel, then follow
  [setup](setup.md). The absence of an update notice in JSON or offline mode does
  not mean the installed version is current.
- **Tracking team source:** `sync` fetches clean tracking sources before
  resolving and materializing. `--dry-run sync` does not fetch them; inspect
  `unrefreshed_tracking_sources` and explain that the preview omits upstream
  changes. A real sync can apply unrelated safe work while another source stays
  degraded. Read the report and verify each requested source.
- **User-managed catalog:** `source refresh <id>` fetches and reports drift but
  does not advance the pin. For an actual update, if supported, inspect
  `--dry-run --json source refresh <id> --advance`, review changed selections and
  blocking findings, then run `source refresh <id> --advance`. This can update
  affected target links directly. Newly required skills may still need approval.
- **Team-manifest catalog:** respect the authority reported by `source list`.
  The pin belongs to the team's `dalo.toml`. A consumer should refresh its team
  source; do not override the pin locally. If the task is to author the team's
  update, use the installed `team catalog update` help and the team's repository
  workflow. Do not commit, push, or publish a team update merely to refresh one
  consumer.

For “check for updates”, report differences without advancing pins or syncing
targets. Upstream checks fetch data; do not start them during a purely local
inventory or when offline. Do not implement a guessed update loop over every
source kind. A request to update all existing skills does not authorize new
source-wide approvals, new hook execution, or accepting audit exceptions.

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

## Repair and resume

| Finding | Response |
| --- | --- |
| Dirty team checkout | Inspect and preserve the diff; do not reset, clean, stash, or commit the user's work automatically |
| Missing or degraded source | Restore access or diagnose the checkout; preserve existing owned links |
| Repointed link or real content at an owned path | Inspect that content and ownership; do not treat it as an ordinary adoptable skill |
| Pending skill approval | Inspect the exact source-qualified skill and audit; approve only within the requested trust scope |
| Blocking security finding | Explain the finding and staged content; do not invent an `--accept-risk` reason |
| Catalog selection removed upstream | Preserve the current pin until the user chooses a replacement or removal |
| Unmanaged conflict | Follow migration; keep, adopt, or explicitly hand over the exact entry |
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

For removal, distinguish a catalog selection (`source unselect`), an entire
source (`source remove`, preview first), and a target (`target unlink`, followed
by sync to remove its owned links). Unlinking a target alone removes no files.
`resolve remove-owned` repairs a recorded link; an active skill can reappear on
the next sync. Do not present that repair as permanent uninstallation or delete
local skill content to hide it from one agent. Report when per-target filtering
or another requested behavior is unsupported instead of editing internal state.

# Troubleshooting and FAQ

This page maps Dalo status and doctor codes to concrete recovery steps. Start with:

```sh
dalo status
dalo doctor
```

Use `--json` when scripting recovery:

```sh
dalo --json status
dalo --json doctor
```

## Fast Recovery Paths

| Symptom or code | What happened | Recovery |
| --- | --- | --- |
| `blocked_by_same_name_skill`, `unmanaged_same_name_blocker`, sync `blocked` with `real unmanaged entry exists at target slot` | A real folder already occupies the target slot Dalo wanted to link. | Keep it intentionally with `dalo resolve keep <id>` (undo with `dalo resolve unkeep <target>:<slot>`), adopt it with `dalo adopt <id>`, or adopt and replace it with `dalo adopt <id> --replace`. |
| `pending_approval` | A skill would become active, but no approval rule covers it. | Review the skill, then run `dalo approve skill <source-id>:<skill>` (or grant a reviewed source/author/org scope), or set the source `trusted = true` if the whole source is trusted. Run `dalo status` again. |
| error `security audit blocked ...` | A selected or trusted skill has an unaccepted high or critical finding. Trust skips per-skill approval; it does not bypass the security gate. | Inspect it with `dalo audit <source-id>:<skill>`. If the risk is understood, record a reason with `dalo audit <source-id>:<skill> --accept-risk "<reason>"`, or use `dalo approve skill <source-id>:<skill> --accept-risk "<reason>"` for a catalog skill. Then rerun `dalo sync`. |
| `dirty_source` | A Git-backed source has local edits. Team sources block refresh when dirty. | Commit, stash, discard, or promote the edits outside Dalo. Then run `dalo sync`. |
| sync reason `scan degraded`, output `degraded source:` | Dalo could not safely scan an enabled source. Recorded owned links are preserved so an incomplete scan cannot delete them. | Restore or re-clone the source checkout, or remove the source with `dalo source remove <id>`. Do not adopt or delete the preserved target link as an unmanaged blocker. |
| `lock drift` | Live resolution differs from the last `lock.toml`. | Run `dalo status` to inspect the drift, then `dalo sync` when the change is expected. |
| `source_provenance_mismatch` | A manifest-derived catalog's declaration, generated config, checkout HEAD, or `source-lock.toml` pin disagree. | Inspect `dalo source list`, review the declaring team's `dalo.toml`, then run `dalo sync` to reconcile an expected change. Restore the reviewed manifest or checkout first when the difference is unexpected. |
| `StoreLocked`, error text `another dalo operation is running` | Another Dalo command currently owns `.lock`, or a stale lock file remains. | Wait for the other command. If no Dalo process is running, inspect and remove the stale `.lock` file in the store. |
| `owned_path_real_entry` | Dalo has an ownership record, but a real file or directory now exists at that path. | Run `dalo resolve remove-owned <id>`. Dalo drops the ownership record and leaves the real entry intact. |
| `missing_owned_symlink`, `broken_owned_symlink`, `foreign_owned_symlink` | A recorded owned symlink is missing, broken, or points outside the store. | Run `dalo resolve remove-owned <id>`, then `dalo sync` if the skill should be linked again. |
| `instruction_block_drift` | A managed instruction block is missing, malformed, stale, or points to a missing pack. | Re-render with `dalo instructions enable <pack> <file>`, or disable with `dalo instructions disable <pack> <file>` if no longer wanted. |
| `selected_removed` from catalog drift | A selected catalog skill disappeared upstream. | Unselect it with `dalo source select <catalog> --unselect <skill>`, or wait for a catalog fix before syncing. |
| catalog drift | A catalog's upstream inventory differs from its pinned snapshot. | Run `dalo source refresh <id>` to inspect it. Add `--check` for CI, or preview the reviewed update with `dalo --dry-run source refresh <id> --advance` before applying it without `--dry-run`. |
| `autosync_disabled` | Dalo has install metadata, but the native scheduler no longer reports the job enabled. | Reinstall idempotently with `dalo autosync install --schedule <hourly\|daily\|weekly>`, or remove it with `dalo autosync uninstall`. |
| `autosync_run_blocked` | The latest scheduled run encountered a dirty source, malformed state, pending approval, audit finding, or target conflict. | Run `dalo autosync status` for the durable reason, resolve it with the suggested normal Dalo command, then retry or wait for the next schedule. |
| `autosync_run_stale` | A scheduled run is still marked `running` well past its schedule interval, so it was likely interrupted (crash or power loss) before recording an outcome. | Run `dalo autosync status` to confirm, then trigger a fresh run (`dalo sync`) or wait for the next schedule to overwrite the state. |
| autosync `skipped` | Another interactive Dalo process held the store lock. | No action is normally required. The next scheduled run retries without contention. |

## Status Codes

### Resolver Diagnostics

These appear in `status.resolution.diagnostics` and in related text output.

| Code | What it means | Recovery |
| --- | --- | --- |
| `pending_approval` | A would-be winner is held until locally approved. | Run `dalo approve skill <source-id>:<skill>`, trust the source, or leave it pending. |
| `local_override` | A local skill wins over another managed source for the same slot. | No action required if intentional. Rename/remove the local skill if the team/catalog skill should win. |
| `shadowed` | A lower-priority managed skill lost to another managed skill with the same slot. | No action required if expected. Adjust source priorities or rename one skill if the loser should be active. |
| `required_expanded` | A selected catalog skill pulled in a same-catalog dependency through `requires`. | No action required if the dependency is expected. Review the dependency before syncing. |
| `cross_source_require` | A `requires` entry points at another source. Dalo reports it but does not auto-install across sources. | Select or add the dependency explicitly, or change the skill metadata to use a same-source requirement. |
| `required_blocked` | A skill was held back because a required closure cannot be linked. | Use the closure block reason below to fix the dependency, approval, shadowing, or target conflict. |
| `legacy_bare_approval` | An older skill approval names only the skill and is no longer accepted because it is ambiguous across sources. | Re-grant it with `dalo approve skill <source-id>:<skill>` as suggested by the diagnostic. |
| `audit_failed` | Dalo could not complete the security audit for one active skill. The owning source is degraded and existing owned links are preserved until the audit succeeds. | Inspect `status.audit_failures`, restore the reported path or permissions, then rerun `dalo status` or `dalo sync`. |

### Required-Closure Block Reasons

These appear in `status` under `blocked skills`.

| Reason | What it means | Recovery |
| --- | --- | --- |
| `missing` | The required reference exists in no enabled source. | Add/select the required skill, enable its source, or remove/fix the `requires` entry. |
| `pending approval` | The required skill exists but is not approved. | Approve the required skill or its source/owner. |
| `shadowed but not satisfied` | The required skill is shadowed by a different winner. | Adjust priorities, rename skills, or require the winner instead. |
| `blocked by a same-name target entry` | The required skill would be blocked by an unmanaged target entry. | Adopt, replace, rename, or keep the unmanaged entry. |
| `unlinked` | The required skill exists but is otherwise not linkable. | Inspect `dalo status` and resolve the related unlinked/blocking reason first. |

### Unlinked Skills

| Reason | What it means | Recovery |
| --- | --- | --- |
| `shadowed` | Another managed source won the same slot name. | No action required if expected. Change source priority, remove/rename one skill, or select a different catalog skill if the loser should win. |

### Lock Drift

Lock drift compares the previous `lock.toml` with the current live resolution.

| Code | What it means | Recovery |
| --- | --- | --- |
| `source_commit_changed` | A source commit changed since the lock was written. | Review source changes, then run `dalo sync` to write a fresh lock. |
| `source_removed` | A source from the lock is no longer configured. | Run `dalo sync` to reconcile, or restore the source config. |
| `source_added` | A new source is configured but absent from the lock. | Run `dalo sync` after reviewing the source. |
| `active_removed` | A previously active skill is no longer active. | Review why it disappeared, then run `dalo sync` if expected. |
| `active_added` | A skill is now active but was not active in the lock. | Review the skill and approvals, then run `dalo sync`. |
| `unlinked_removed` | A previously unlinked skill is no longer unlinked. | Usually no action; run `dalo sync` if the new resolution is expected. |
| `unlinked_added` | A skill is now unlinked. | Inspect the reason, usually shadowing, then adjust priority/selection/name if needed. |
| `pending_approval_removed` | A previously pending skill is no longer pending. | Usually no action; run `dalo sync` if the new resolution is expected. |
| `pending_approval_added` | A skill now needs approval. | Approve it or leave it pending. |

### Inventory Warnings

| Code | What it means | Recovery |
| --- | --- | --- |
| `malformed_frontmatter` | `SKILL.md` frontmatter could not be parsed. | Fix the YAML frontmatter fences and fields, then rerun `dalo status`. |
| `invalid_slot_name` | Frontmatter `name` or folder name is not a portable slot name. | Rename the folder or frontmatter `name` to a lowercase portable token. |
| `duplicate_slot_name` | One source contains multiple skills with the same slot name. | Rename one skill or split the source. |
| `unreadable_path` | Dalo could not read a skill path. | Fix filesystem permissions, broken links, or the source checkout. |
| `skipped_symlink` | Dalo skipped a symlinked directory to keep source discovery inside a bounded tree and avoid cycles. | Replace it with a real in-tree directory if it should contain skills, or remove the symlink. |

### Target Scan Warnings

| Code | What it means | Recovery |
| --- | --- | --- |
| `unreadable_target_dir` | A linked target directory or child entry could not be scanned. | Fix permissions or remove the unreadable entry, then rerun `dalo status` or `dalo doctor`. |

### Sync Operations

`dalo sync --json` reports materialization operations.

| Kind/status | What it means | Recovery |
| --- | --- | --- |
| `create` / `applied` | Dalo created an owned symlink. | No action. |
| `relink` / `applied` | Dalo moved an owned symlink to the desired store path. | No action. |
| `remove` / `applied` | Dalo removed a no-longer-desired owned symlink. | No action. |
| `drop_record` / `applied` | Dalo dropped stale ownership state without touching a real entry. | No action unless the skill should be linked again; then run `dalo sync`. |
| `conflict` / `blocked`, reason says a real entry or foreign symlink occupies the slot | Dalo refused to touch unmanaged or foreign content. | Adopt, keep, rename, or remove the blocker, then rerun `dalo sync`. |
| `conflict` / `blocked`, reason contains `source <id> scan degraded` | Dalo preserved a recorded owned link because its source could not be scanned safely. | Restore or re-clone the source checkout, or run `dalo source remove <id>`. Leave the preserved target link in place. |
| any kind / `planned` | `--dry-run` showed what would happen. | Rerun without `--dry-run` if the plan is correct. |
| any kind / `existing` | The filesystem already matched the desired state. | No action. |

### Catalog Drift

| Code | What it means | Recovery |
| --- | --- | --- |
| `new_available` | Upstream added an unselected catalog skill. | Inspect/select it if wanted. |
| `selected_changed` | A selected skill changed content or metadata upstream. | Review `dalo --dry-run source refresh <id> --advance`, then apply the reviewed pin. |
| `selected_moved` | A selected skill moved but still has a stable ID. | Review the advance plan. A real advance canonicalizes the selection to the stable ID and relinks owned targets transactionally. |
| `selected_removed` | A selected skill no longer exists upstream. | Unselect it, restore it upstream, or keep the old pin until resolved. |

### Instruction Block Drift

| Kind | What it means | Recovery |
| --- | --- | --- |
| `missing` | The target file or managed block is missing. | Re-render with `dalo instructions enable <pack> <file>`, or disable the pack if no longer wanted. |
| `malformed` | Managed block markers are duplicated, reversed, unreadable, or malformed. | Fix the markers manually, then re-run `dalo instructions enable <pack> <file>`. |
| `stale` | The block exists but no longer matches the pack body. | Re-render with `dalo instructions enable <pack> <file>`. |
| `source_missing` | The active lock entry points to a pack that cannot be read. | Restore the pack file or disable the pack. |

## Doctor Findings

Doctor includes `ok` and `info` codes as well as warnings/errors. Codes not listed as requiring action are informational.

| Code | Severity | Recovery |
| --- | --- | --- |
| `store_missing` | error | Run `dalo init` or pass the right `--store`. |
| `store_layout_missing` | error | Run `dalo init` to recreate missing store paths. |
| `config_invalid` | error | Fix `config.toml` or restore it from version control/backups. |
| `state_invalid` | error | Run `dalo init`; corrupt state is backed up and regenerated. Relink targets afterward if needed. |
| `lock_invalid` | error | Fix/remove `lock.toml`, then run `dalo sync` to regenerate it. |
| `source_lock_invalid` | error | Inspect or restore `source-lock.toml`; do not sync until the intended catalog pins are understood. |
| `source_provenance_mismatch` | error | Compare `dalo source list` with the declaring team's `dalo.toml`, then run `dalo sync` after restoring the intended declaration or checkout. |
| `source_store_debris` | warning | Inspect and remove the reported unconfigured source content or interrupted-operation directory when it is no longer needed. |
| `approvals_invalid` | error | Fix `approvals.toml`; doctor suppresses approval-dependent warnings while it is invalid. |
| `git_missing` | error | Install Git and ensure `git` is on `PATH`. |
| `gh_missing` | warning | Install GitHub CLI if you need future PR/promotion flows. Normal sync does not require it. |
| `gh_unauthenticated` | warning | Run `gh auth login` if you need GitHub PR flows. |
| `local_git_missing` | error | Run `dalo init` to restore the local source Git repository. |
| `target_missing` | warning | Recreate the directory or run `dalo target link <target> [path]`. |
| `cloud_synced_target` | warning | Prefer a non-cloud-synced target path if sync software interferes with symlinks. |
| `foreign_owned_symlink` | error | Run `dalo resolve remove-owned <id>`. |
| `broken_owned_symlink` | error | Run `dalo resolve remove-owned <id>`, then `dalo sync` if it should be recreated. |
| `owned_path_real_entry` | error | Run `dalo resolve remove-owned <id>`; the real entry stays in place. |
| `missing_owned_symlink` | warning | Run `dalo resolve remove-owned <id>`, then `dalo sync` if needed. |
| `dirty_source` | error for team/catalog, warning for local | The checkout has local edits to tracked files (untracked files such as `.DS_Store` no longer count). Commit, stash, discard, or intentionally keep them. |
| `source_missing` | error | The enabled source's checkout is missing from disk. Restore/re-clone it or run `dalo source remove <id>`. |
| `pending_approval` | warning | Add the needed approval or leave the skill pending. |
| `required_closure_blocked` | error | Resolve the closure block reason shown in the message. |
| `instruction_pack_topic_overlap` | warning | Rename topics or disable one overlapping pack if the overlap is not intended. |
| `instruction_block_drift` | error | Re-render or disable the pack. |
| `autosync_installed` | ok | The native scheduler is installed and enabled. |
| `autosync_not_installed` | info | Install it with `dalo autosync install` if recurring synchronization is desired. |
| `autosync_disabled` | warning | Reinstall with `dalo autosync install`, or remove stale metadata with `dalo autosync uninstall`. |
| `autosync_executable_missing` | warning | The executable path recorded during installation is missing or no longer executable. Reinstall with `dalo autosync install` from a persistent launcher. |
| `autosync_run_blocked` | warning | Inspect `dalo autosync status`, resolve its recorded reason, and retry. |
| `autosync_run_stale` | warning | A run is still `running` long after it started; it was likely interrupted. Trigger a fresh `dalo sync` or wait for the next schedule. |
| `autosync_state_invalid` | error | Run `dalo autosync uninstall` to quarantine malformed or newer-schema `autosync.toml` and clean reconstructed scheduler artifacts, then reinstall. Repair or remove malformed `autosync-run.toml` separately. |
| `unreadable_target_directory` | warning | Fix permissions or remove unreadable entries. |
| `unmanaged_same_name_blocker` | error | Adopt, keep, rename, or remove the unmanaged blocker. |
| `stale_protected_skill` | warning | Relink the target if it moved, or remove the stale marker with the suggested `dalo resolve unkeep` command. |
| `protected_skill_kept` | info | The unmanaged slot was intentionally kept; no recovery is required. |
| `store_exists`, `store_layout_ok`, `config_ok`, `state_ok`, `lock_ok`, `approvals_ok`, `git_available`, `gh_available`, `gh_authenticated`, `local_git_ok`, `target_exists`, `duplicate_target_directory`, `owned_symlink_ok`, `source_clean` | ok/info | No recovery required. |

## FAQ

### Why did `sync` not overwrite my folder?

Dalo treats real folders in target directories as user/project content. Use `dalo adopt <id>` to copy the folder into the local source, `dalo adopt <id> --replace` to replace it with an owned symlink, or `dalo resolve keep <id>` to leave it intentionally unmanaged without failing checks. Undo that decision with `dalo resolve unkeep <target>:<slot>`.

### How do I approve a pending skill?

Review the reported source-qualified skill, then run:

```sh
dalo approve skill <source-id>:<skill>
```

Use `dalo approve list` to inspect existing rules and [the user reference](reference.md#dalo-approve) for broader source, author, and organization scopes. Rerun `dalo status` before syncing.

### How do I recover from a security-audit block?

An audit block means deterministic or optional semantic review found an
unaccepted `high` or `critical` risk. Semantic review is optional and can add
findings, but a review with no additional findings is not an approval or safety
guarantee. This applies to trusted team sources too:
trust removes the catalog-style approval requirement, but it never bypasses the
security gate.

Inspect the exact source-qualified skill first:

```sh
dalo audit <source-id>:<skill>
```

If the behavior is expected and you accept the risk, record a specific reason:

```sh
dalo audit <source-id>:<skill> --accept-risk "reviewed pinned installer"
```

For a catalog skill that still needs approval, combine both decisions:

```sh
dalo approve skill <source-id>:<skill> \
  --accept-risk "reviewed pinned installer"
```

Then rerun `dalo sync`. Acceptance is bound to the source, exact content,
engine versions, coverage, and findings; a relevant change invalidates it and
requires a fresh review.

### How do I recover from a dirty team source?

Go to the checkout shown by `dalo doctor`, then commit, stash, or discard the edits with normal Git commands. Dalo does not decide this for you because the edits may be user or agent work.

### How do I remove Dalo completely?

Use the uninstall guide: [Uninstall Dalo](uninstall.md).

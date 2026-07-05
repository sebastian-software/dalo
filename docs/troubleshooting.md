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
| `blocked_by_same_name_skill`, `unmanaged_same_name_blocker`, sync `blocked` with `real unmanaged entry exists at target slot` | A real folder already occupies the target slot Dalo wanted to link. | Keep it with `dalo resolve keep <id>`, adopt it with `dalo adopt <id>`, or adopt and replace it with `dalo adopt <id> --replace`. |
| `pending_approval` | A skill would become active, but no approval rule covers it. | Review the skill, then add a source/skill/owner approval in `approvals.toml`, or set the source `trusted = true` if the whole source is trusted. Run `dalo status` again. |
| `dirty_source` | A Git-backed source has local edits. Team sources block refresh when dirty. | Commit, stash, discard, or promote the edits outside Dalo. Then run `dalo sync`. |
| `lock drift` | Live resolution differs from the last `lock.toml`. | Run `dalo status` to inspect the drift, then `dalo sync` when the change is expected. |
| `StoreLocked`, error text `another dalo operation is running` | Another Dalo command currently owns `.lock`, or a stale lock file remains. | Wait for the other command. If no Dalo process is running, inspect and remove the stale `.lock` file in the store. |
| `owned_path_real_entry` | Dalo has an ownership record, but a real file or directory now exists at that path. | Run `dalo resolve remove-owned <slot>`. Dalo drops the ownership record and leaves the real entry intact. |
| `missing_owned_symlink`, `broken_owned_symlink`, `foreign_owned_symlink` | A recorded owned symlink is missing, broken, or points outside the store. | Run `dalo resolve remove-owned <slot>`, then `dalo sync` if the skill should be linked again. |
| `instruction_block_drift` | A managed instruction block is missing, malformed, stale, or points to a missing pack. | Re-render with `dalo instructions enable <pack> <file>`, or disable with `dalo instructions disable <pack> <file>` if no longer wanted. |
| `selected_removed` from catalog drift | A selected catalog skill disappeared upstream. | Unselect it with `dalo source select <catalog> --unselect <skill>`, or wait for/source a catalog fix before syncing. |
| `source refresh (advancing the pin)` is not implemented | `dalo source refresh <id>` was run without `--check`. | Use `dalo source refresh <id> --check`. Pin advancement is a later workflow. |

## Status Codes

### Resolver Diagnostics

These appear in `status.resolution.diagnostics` and in related text output.

| Code | What it means | Recovery |
| --- | --- | --- |
| `pending_approval` | A would-be winner is held until locally approved. | Add an approval in `approvals.toml`, trust the source, or leave it pending. |
| `local_override` | A local skill wins over another managed source for the same slot. | No action required if intentional. Rename/remove the local skill if the team/catalog skill should win. |
| `shadowed` | A lower-priority managed skill lost to another managed skill with the same slot. | No action required if expected. Adjust source priorities or rename one skill if the loser should be active. |
| `required_expanded` | A selected catalog skill pulled in a same-catalog dependency through `requires`. | No action required if the dependency is expected. Review the dependency before syncing. |
| `cross_source_require` | A `requires` entry points at another source. Dalo reports it but does not auto-install across sources. | Select or add the dependency explicitly, or change the skill metadata to use a same-source requirement. |
| `required_blocked` | A skill was held back because a required closure cannot be linked. | Use the closure block reason below to fix the dependency, approval, shadowing, or target conflict. |

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
| `conflict` / `blocked` | Dalo refused to touch unmanaged or foreign content. | Adopt/keep/rename/remove the blocker, then rerun `dalo sync`. |
| any kind / `planned` | `--dry-run` showed what would happen. | Rerun without `--dry-run` if the plan is correct. |
| any kind / `existing` | The filesystem already matched the desired state. | No action. |

### Catalog Drift

| Code | What it means | Recovery |
| --- | --- | --- |
| `new_available` | Upstream added an unselected catalog skill. | Inspect/select it if wanted. |
| `selected_changed` | A selected skill changed content or metadata upstream. | Review the change before advancing the pin in a future source-maintenance flow. |
| `selected_moved` | A selected skill moved but still has a stable ID. | Review the move. Selection can continue by stable ID. |
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
| `approvals_invalid` | error | Fix `approvals.toml`; doctor suppresses approval-dependent warnings while it is invalid. |
| `git_missing` | error | Install Git and ensure `git` is on `PATH`. |
| `gh_missing` | warning | Install GitHub CLI if you need future PR/promotion flows. Normal sync does not require it. |
| `gh_unauthenticated` | warning | Run `gh auth login` if you need GitHub PR flows. |
| `local_git_missing` | error | Run `dalo init` to restore the local source Git repository. |
| `target_missing` | warning | Recreate the directory or run `dalo target link <target> [path]`. |
| `cloud_synced_target` | warning | Prefer a non-cloud-synced target path if sync software interferes with symlinks. |
| `foreign_owned_symlink` | error | Run `dalo resolve remove-owned <slot>`. |
| `broken_owned_symlink` | error | Run `dalo resolve remove-owned <slot>`, then `dalo sync` if it should be recreated. |
| `owned_path_real_entry` | error | Run `dalo resolve remove-owned <slot>`; the real entry stays in place. |
| `missing_owned_symlink` | warning | Run `dalo resolve remove-owned <slot>`, then `dalo sync` if needed. |
| `dirty_source` | error for team/catalog, warning for local | Commit, stash, discard, or intentionally keep local work before syncing. |
| `pending_approval` | warning | Add the needed approval or leave the skill pending. |
| `required_closure_blocked` | error | Resolve the closure block reason shown in the message. |
| `instruction_pack_topic_overlap` | warning | Rename topics or disable one overlapping pack if the overlap is not intended. |
| `instruction_block_drift` | error | Re-render or disable the pack. |
| `unreadable_target_directory` | warning | Fix permissions or remove unreadable entries. |
| `unmanaged_same_name_blocker` | error | Adopt, keep, rename, or remove the unmanaged blocker. |
| `store_exists`, `store_layout_ok`, `config_ok`, `state_ok`, `lock_ok`, `approvals_ok`, `git_available`, `gh_available`, `gh_authenticated`, `local_git_ok`, `target_exists`, `duplicate_target_directory`, `owned_symlink_ok`, `source_clean` | ok/info | No recovery required. |

## FAQ

### Why did `sync` not overwrite my folder?

Dalo treats real folders in target directories as user/project content. Use `dalo adopt <id>` to copy the folder into the local source, `dalo adopt <id> --replace` to replace it with an owned symlink, or `dalo resolve keep <id>` to leave it unmanaged.

### How do I approve a pending skill?

Edit `approvals.toml` and add a `skill`, `source`, `author`, or `org` record. The exact schema is in [the user reference](reference.md#approvalstoml). Rerun `dalo status` before syncing.

### How do I recover from a dirty team source?

Go to the checkout shown by `dalo doctor`, then commit, stash, or discard the edits with normal Git commands. Dalo does not decide this for you because the edits may be user or agent work.

### How do I remove Dalo completely?

Use the uninstall guide: [Uninstall Dalo](uninstall.md).

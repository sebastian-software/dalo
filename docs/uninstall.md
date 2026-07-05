# Uninstall Dalo

This guide removes Dalo without leaving broken Dalo-owned symlinks in agent folders.

## 1. Inspect Current State

Run:

```sh
dalo target detect
dalo status
dalo doctor
```

If `doctor` reports broken/foreign/missing owned symlinks or real entries at owned paths, clear those records first:

```sh
dalo resolve remove-owned <slot>
```

That command removes Dalo-owned symlinks or drops stale ownership records. It does not delete real user directories.

## 2. Unlink Targets

Unlink each target Dalo knows about:

```sh
dalo target unlink codex
dalo target unlink claude
dalo target unlink openclaw
dalo target unlink hermes
dalo target unlink generic
```

Only run the targets you actually linked. `target unlink` updates Dalo state. It does not delete real files from the target directory.

Then run `sync` once so Dalo removes the owned symlinks that are no longer desired:

```sh
dalo sync
```

`sync` removes Dalo-owned symlinks and stale ownership records. It does not delete unmanaged real directories.

Run `dalo status` again and confirm `sync` no longer reports owned target entries.

## 3. Disable Instruction Packs

If you enabled instruction packs, remove their managed blocks:

```sh
dalo instructions list
dalo instructions disable <pack> <instruction-file>
```

Dalo only removes the pack's managed block. Content outside Dalo markers is preserved.

## 4. Remove the Store

After target symlinks and instruction blocks are removed, delete the store directory:

```sh
rm -rf ~/.dalo
```

Use the path from `dalo status` if you used `--store` or `DALO_STORE`.

Adopted skills live in the local source under the store. Back up anything in `local/skills/` or `local/instructions/` before deleting the store if you want to keep it.

## 5. Uninstall the Binary

If installed through Cargo:

```sh
cargo uninstall dalo
```

If installed from a GitHub release archive, remove the copied `dalo` binary from wherever you placed it on `PATH`.

## 6. Final Check

Inspect the agent folders you had linked, such as:

```sh
ls -la ~/.agents/skills
ls -la ~/.claude/skills
ls -la ~/.hermes/skills
```

Remove only broken symlinks that point into the deleted Dalo store. Leave real directories and project/user-authored files in place.

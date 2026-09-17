# Upgrading to 1.0

This page is for someone who already runs Dalo 0.x and is moving to 1.0. It
answers four questions: do I have to do anything, what does the first run
print, which spellings are gone, and what happens if I have to go back.

> **Release notes: [what 1.0 changes, and
> why](https://github.com/sebastian-software/dalo/releases/tag/dalo-v1.0.0)** —
> the highlights since 0.6, the breaking changes with their replacements, and
> the same upgrade summary in short. The identical text opens the 1.0.0 entry in
> [CHANGELOG.md](../CHANGELOG.md).

**Short answer: install the new binary and run `dalo sync`. There is no
migration command, nothing is unlinked, and no approval has to be granted
again.** The rest of this page is what you see while that happens, plus the
four CLI spellings that were removed before the 1.x line froze.

## Supported upgrade paths

Any store written by Dalo **0.6.0 or later** opens in 1.0 with no manual steps.
There is no intermediate version you have to install on the way.

This is not a claim; it is a test. `tests/upgrade.rs` restores a store written
by the *released* 0.6.0, 0.9.2, 0.12.0, and 0.15.1 binaries into a temporary
root and runs what a person runs after upgrading — `status`, `doctor --check`,
`sync --dry-run`, `sync`, `sync` again, `status --json`. For every fixture it
asserts that the resolved skill set is unchanged, that no owned symlink is
removed or repointed, that an unmanaged directory is left alone, that the
second `sync` is a pure no-op, and that `doctor --check` exits `0`. The
fixtures live in
[`tests/fixtures/stores/`](https://github.com/sebastian-software/dalo/blob/main/tests/fixtures/stores/README.md)
and every persisted file in them is byte for byte what the released binary
wrote.

A store older than 0.6.0 is not covered by a fixture. Dalo still carries the
read paths for it, but the honest answer is that nobody tests it — take a copy
of the store first.

## What the first run does

Migration is lazy. A newer Dalo reads an older file, upgrades it in memory, and
persists the current version the next time it writes that file at all. Because
of that, a file no command has written yet keeps its old version on disk, which
is correct but invisible — so `dalo doctor` prints one `schema_migration_pending`
line per file that will migrate on its next write.

The transcript below is real. It was produced by building this tree
(`cargo build`), restoring the 0.9.2 fixture store the way `tests/upgrade.rs`
does, and running `doctor`, `sync`, `doctor`. Only the throwaway root is
shortened, to `…`; every other character is what the binary printed.

### Before the first sync

```text
$ dalo doctor
summary: errors=0 warnings=1 info=5 ok=22
info    schema_migration_pending: `config.toml` is at schema version 1 and is read as 2; the next config write persists the migration
info    schema_migration_pending: `lock.toml` is at schema version 1 and is read as 6; the next `dalo sync` persists the migration
info    schema_migration_pending: instruction pack `local:house-style` renders a legacy managed block in `…/AGENTS.md`; the next `dalo sync` rewrites it without pack metadata
details: 24 info/ok findings omitted; use --json for the full report
```

`doctor` exits `0`. These are `info` findings, not problems: nothing is broken
and nothing is waiting for you.

### The sync itself

```text
$ dalo sync
existing noop       target[generic]:/copy-editing -> store:/sources/public/checkout/skills/copy-editing
existing noop       target[generic]:/local-note -> store:/local/skills/local-note
existing noop       target[generic]:/team-review -> store:/sources/team/checkout/skills/team-review
synced: 3 skills across 1 target (3 unchanged)
security preflight: deterministic checks only
```

Every link is `noop`. The migration happened inside the files Dalo owns; the
agent folder did not move.

### After the first sync

```text
$ dalo doctor
summary: errors=0 warnings=1 info=3 ok=22
info    schema_migration_pending: `config.toml` is at schema version 1 and is read as 2; the next config write persists the migration
details: 24 info/ok findings omitted; use --json for the full report
```

`lock.toml` and the instruction block are done. `config.toml` is still on
version 1 because **`sync` does not write `config.toml`** — it migrates the
first time you change a setting, add a source, or link a target. Leaving it is
safe; the line is there so the state is visible rather than silent.

### Which file migrates when

| File | Migrates on | Reported by `doctor` until then |
| --- | --- | --- |
| `lock.toml` (1–5 → 6) | the next `dalo sync` | yes |
| `state.toml` protected slot recorded as a path | the next state write, which `sync` performs | yes |
| An instruction managed block that still carries pack metadata | the next `dalo sync` | yes |
| `config.toml` (1 → 2) | the next config write: a setting, a source, a target | yes |
| `source-lock.toml` (1–2 → 3) | the next catalog write: `source select`, `source unselect`, or `source refresh` | yes |

A store carried since 0.6.0 shows all five at once, plus one warning that is
**not** automatic, because only a person can decide what the record meant:

```text
warning legacy_approval_record: legacy approval `launch-copy` found for `public:launch-copy`; re-approve as `public:launch-copy` next=dalo approve skill public:launch-copy
info    schema_migration_pending: `config.toml` is at schema version 1 and is read as 2; the next config write persists the migration
info    schema_migration_pending: `lock.toml` is at schema version 1 and is read as 6; the next `dalo sync` persists the migration
info    schema_migration_pending: `source-lock.toml` is at schema version 2 and is read as 3; the next catalog write persists the migration
info    schema_migration_pending: instruction pack `local:house-style` renders a legacy managed block in `…/AGENTS.md`; the next `dalo sync` rewrites it without pack metadata
info    schema_migration_pending: protected slot `keep-mine` is recorded as an absolute path and is read as a target slot; the next state write persists the migration
```

An approval that names only a skill, without its source, is ambiguous across
sources. Dalo keeps the skill pending and names the exact command to re-grant
it; run that command once and the warning is gone. Every other line clears
itself.

The full list of older shapes Dalo accepts on read is
[Accepted 0.x Store Shapes](reference.md#accepted-0x-store-shapes).

## Removed flags and spellings

Four CLI spellings and one target ID were removed before 1.0 froze the surface,
so that no deprecated spelling enters the 1.x line. Each one now fails loudly
instead of quietly doing something else. The error lines below are what the
current binary actually prints.

| Removed | Replacement | What you see now | Exit |
| --- | --- | --- | --- |
| `--yes` (global) | none — drop it | `error: unexpected argument '--yes' found` | `2` |
| `audit --agent <reviewer>` | `audit --reviewer <reviewer>` | `error: unexpected argument '--agent' found` | `2` |
| `source select <id> --unselect <skill>...` | `source unselect <id> <skill>...` | `error: unexpected argument '--unselect' found` | `2` |
| `--refresh` (`audit`, `adopt`, `approve skill`, `resolve adopt`) | `--refresh-audit` | `error: unexpected argument '--refresh' found` | `2` |
| target ID `cursor` | `dalo target link generic ~/.cursor/skills` | ``error: unknown target `cursor`; known targets: claude, codex, generic, hermes, openclaw, opencode`` | `1` |

Two of them come with clap's own hint, which is usually enough to fix a script
without reading anything else:

```text
$ dalo audit public:copy-editing --refresh
error: unexpected argument '--refresh' found

  tip: a similar argument exists: '--refresh-audit'

Usage: dalo audit --refresh-audit <SKILL>
```

```text
$ dalo source select public --unselect skills/copy-editing
error: unexpected argument '--unselect' found

  tip: to pass '--unselect' as a value, use '-- --unselect'

Usage: dalo source select <ID> <SKILLS>...
```

`--yes` never confirmed a prompt, implied `--replace`, created a commit, or
granted an approval — it was documented as a no-op. Removing it changes no
behavior beyond the new error, so a script that passed it can simply stop.

### About Cursor

The `cursor` target ID is gone because symlink discovery could not be verified
against a current Cursor release, and Dalo does not ship an unverified promise.
Nothing about your setup has to change:

```sh
dalo target link generic ~/.cursor/skills
```

That materializes into exactly the same directory. Cursor also reads
`~/.agents/skills`, so a linked `codex` or `openclaw` target already reaches it
without a second link.

## Other changes a 0.x user notices

Only behavior confirmed against this tree is listed here.

- **`doctor` prints migration lines in text mode.** `schema_migration_pending`
  is exempt from the usual "info findings omitted" summary, because it
  describes a change Dalo is going to make to your own files. It is an `info`
  finding, so it does not affect `doctor --check`.
- **Instruction managed blocks lose their pack metadata.** The next `sync`
  re-renders the block without it — but only when the block still matches
  Dalo's exact older rendering byte for byte. A block you edited by hand is
  left alone and reported instead.
- **A bare skill approval stays pending.** See the warning above; it was
  already rejected in 0.6.0, and 1.0 now names the command that fixes it.
- **`opencode` is a supported built-in target** with the default path
  `~/.config/opencode/skills`, and no built-in target is labelled experimental
  any more.
- **A protected unmanaged skill is re-recorded as a target slot.** A 0.6.0
  store recorded it as an absolute path; `sync` rewrites it so a later `sync`
  cannot mistake the slot for an orphan and unlink it. Nothing on disk moves.

## Downgrade

Downgrade is not supported, and Dalo fails closed rather than pretending
otherwise. An older binary that meets a file written by a newer one refuses to
read it. It never truncates the file, ignores fields it does not know, or
rewrites it at the version it understands.

How far back you go decides what you see. **No persisted schema version changed
between 0.15.1 and 1.0**, so going back to 0.15.1 still reads a store this
binary wrote — `status` and `sync` both work. Go back further and the refusal
is explicit. This is the released 0.12.0 binary against a store the current
binary had just synced:

```text
$ dalo status
error: unsupported schema version 6 in `…/lock.toml`; this dalo supports version 4; upgrade dalo
```

`status` and `sync` exit `1` and write nothing. `doctor` still exits `0`,
because it is a report, and surfaces the same refusal as an error finding:

```text
error   lock_invalid: user lock could not be read: unsupported schema version 6 in `…/lock.toml`; this dalo supports version 4; upgrade dalo next=dalo sync
error: check failed: doctor found 1 error findings
```

`doctor --check` exits `1`. Afterwards `lock.toml` still reads
`schema_version = 6`: the store was not modified.

Before 0.12.0 the file is rejected while parsing instead, because those
releases reject unknown fields before they reach the version check. The outcome
is identical — exit `1`, nothing written.

**To recover, upgrade Dalo again.** The store was not touched, so the newer
binary picks up exactly where it left off. If you must stay on the older
version, restore the store from a backup taken before the newer binary ran;
there is no supported way to convert a newer store back.

## Where the promise lives

- [Compatibility and stability](compatibility.md) — what 1.x guarantees, what
  is experimental, and the change policy that governs every future removal.
  Its [Upgrade and downgrade](compatibility.md#upgrade-and-downgrade) section
  is the contract this page demonstrates.
- [Security overview](security.md) — the trust boundaries, the approval model,
  and what the preflight does and does not block.
- [Troubleshooting and FAQ](troubleshooting.md) — every diagnostic code with
  the command that clears it.

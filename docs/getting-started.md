# Getting Started

This guide takes one machine from a fresh install to agents that read the
team's skills. It is written for the developer side. If you own the repository
those skills come from, read the [team repository guide](team.md) instead — or
afterwards.

Prefer to work through your agent? Ask it to **“Install the Dalo skill manager.”**
The [conversational setup](assistant.md) includes the assistant, which can then
help you migrate or maintain your skills.

Before you start:

- install Dalo (see the [README installation instructions](../README.md#installation))
- have `git` on `PATH`
- use Linux or macOS

If you would rather try everything without touching your agent folders, jump to
[Explore without touching your agent folders](#explore-without-touching-your-agent-folders)
and come back.

## 1. Initialize the store

```sh
dalo init
```

The store is Dalo's local database: source checkouts, the private local source,
lockfiles, approvals, and target state. It lives in `~/.dalo` unless `--store`
or `DALO_STORE` says otherwise. Your agent folders are output, never input;
nothing in them is read back into the store by accident.

`init` finishes by printing the next three steps, which are the next three
sections of this page.

## 2. Connect an agent

Ask Dalo what it can see:

```sh
dalo target detect
```

The report lists every known target, the path it would use, whether that path
exists, and whether it is already linked. Then link the agents you use:

```sh
dalo target link claude
# or: dalo target link codex
# or: dalo target link openclaw
# or: dalo target link hermes
# or: dalo target link opencode
```

A known target uses its default directory; pass a path to override it. The
`generic` target always needs an explicit path:

```sh
dalo target link generic /path/to/agent/skills
```

Linking records a directory. It does not move or delete anything yet.

## 3. Add your team's skill repository

A source is a Git-backed collection of skills. A team source is trusted by
default, so its skills do not need per-skill approval:

```sh
dalo source add company git@github.com:acme/agent-skills.git
```

Dalo clones it, discovers the skills under `skills/`, and runs a deterministic
security preflight over every one of them before anything is linked.

If your team repository also pins public catalogs in a `dalo.toml` manifest,
those arrive automatically as `company.<catalog-id>` sources. They are
untrusted, so their skills wait for your approval — see
[pending approval](#pending-approval) below.

## 4. Sync

```sh
dalo sync
```

`sync` refreshes clean tracking sources, resolves one approved skill set, and
links that set into every linked target:

```text
target[claude]: …/skills
applied  create     target[claude]:/incident-review -> store:/sources/company/checkout/skills/incident-review
applied  create     target[claude]:/release-notes -> store:/sources/company/checkout/skills/release-notes
synced: 2 skills across 1 target (2 created)
security preflight: deterministic checks only
```

Dalo creates symlinks it owns. It never overwrites an unmanaged directory or a
symlink it did not create; anything it cannot do safely is reported instead.

Your agent now reads those skills from the folder it already uses.

## 5. Read the state

Two commands answer two different questions.

```sh
dalo status
dalo next
```

`status` is the full picture: sources, targets, active skills, pending
approvals, unlinked skills, materialization blocks, resolution diagnostics, and
lock drift. `dalo status --check` exits non-zero when anything needs attention,
which is what you want in a script or a pipeline.

`next` is the opposite: one summary and exactly one copyable command.

```text
Dalo
  store: …
  initialized: yes
  linked targets: 1
  sources: 4
  active skills: 4
  pending approvals: 0

Next: dalo status
  Linked targets contain unmanaged skills; review detailed status before synchronizing.
```

In a terminal, bare `dalo` prints the same summary. Use `dalo next` explicitly
when output is piped or scripted.

## 6. Three words status uses, and the command that clears each

The first time `status` says something surprising, it is almost always one of
these three. The transcripts below are real output from a linked `claude`
target.

### Pending approval

A skill is available but untrusted, so Dalo will not link it. Catalog sources
and catalogs pinned by a team manifest start here by design.

```text
pending approval:
  review-helper -> public:review-helper (run: dalo approve skill public:review-helper)
```

`sync` still does all the other work and repeats the same line at the end:

```text
synced: 2 skills across 1 target (2 unchanged)
pending approval: public:review-helper (run: dalo approve skill public:review-helper)
security preflight: deterministic checks only
```

Read the skill first — `dalo audit public:review-helper` prints the deterministic
findings and the exact content hash — then run the command status gave you:

```sh
dalo approve skill public:review-helper
dalo sync
```

Approvals are per skill, per machine, and bound to exact content. They are
never inherited from a teammate and never granted by a sync.

### Shadowed

Two sources offer the same skill name. Dalo links exactly one of them and keeps
the other visible instead of discarding it:

```text
unlinked skills:
  release-notes -> company:release-notes reason=shadowed by=personal:release-notes
resolution diagnostics:
  shadowed: skill `company:release-notes` is unlinked because `personal:release-notes` wins the same slot
```

The lower priority number wins. To flip the winner, change one priority:

```sh
dalo source priority company 1
dalo sync
```

If both variants should stay installed side by side, give one source a
namespace instead (`dalo source namespace public acme`); its skills are linked
as `acme__release-notes`.

### Conflict

Dalo wants a slot that is already occupied by a real, unmanaged directory —
usually a skill an agent wrote directly into its own folder.

```text
materialization blocks:
  target[claude]:/standup-notes: real unmanaged entry exists at target slot
unmanaged skills:
  standup-notes -> target[claude]:/standup-notes (adopt: run `dalo adopt 'standup-notes'` to copy it into the local source; use `dalo adopt 'standup-notes' --replace` to replace the original)
```

`sync` reports the block and leaves your file alone:

```text
blocked  conflict   target[claude]:/standup-notes -> store:/sources/company/checkout/skills/standup-notes (real unmanaged entry exists at target slot) …
synced: 4 skills across 1 target (3 unchanged, 1 blocked)
```

Pick one:

```sh
dalo adopt standup-notes --replace   # keep your version, let Dalo manage it
dalo resolve keep standup-notes      # keep it unmanaged and stop reporting it
```

`dalo adopt standup-notes` without `--replace` copies the skill into the local
source and leaves the original directory exactly where it is.

## 7. Keep it current

Rerun `dalo sync` whenever the team repository moves. To let the machine do it:

```sh
dalo autosync install --schedule daily
dalo autosync status
```

macOS uses launchd, Linux a systemd user timer with a marked crontab fallback.
Scheduled runs never grant approvals and never wait on an interactive Dalo
process; whatever they skip or block on stays visible in `status` and `doctor`.

For a focused health check:

```sh
dalo doctor
```

## Explore without touching your agent folders

Everything above also works in a throwaway store with a temporary folder as the
target. Nothing in this section writes to a real agent directory.

### A sandbox store

```sh
export DALO_STORE="$(mktemp -d)/store"
target_dir="$(mktemp -d)/skills"

dalo init
dalo target link generic "$target_dir"
```

### A local skill

```sh
mkdir -p "$DALO_STORE/local/skills/review"
cat > "$DALO_STORE/local/skills/review/SKILL.md" <<'EOF'
# Review

Check behavioral regressions before style nits.
EOF

dalo status
dalo sync
ls -la "$target_dir"
```

The local source is private to your machine: experiments and overrides live
there. `status` may show a `lock drift` block until the next `sync` records the
new skill; see [Lock Drift](troubleshooting.md#lock-drift).

### A local team source

A local Git repository works as a source, so this needs no network access:

```sh
TEAM_REPO="$(mktemp -d)/team-skills"
mkdir -p "$TEAM_REPO/skills/release-notes"
cat > "$TEAM_REPO/skills/release-notes/SKILL.md" <<'EOF'
# Release Notes

Summarize user-visible changes first.
EOF

git -C "$TEAM_REPO" init
git -C "$TEAM_REPO" add .
git -C "$TEAM_REPO" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -m initial

dalo source add company "$TEAM_REPO"
dalo source list
dalo sync
```

Sync still audits team content and blocks unaccepted high or critical findings.

### A local catalog source

A catalog offers many skills and installs only what you select. This is where
[pending approval](#pending-approval) comes from:

```sh
CATALOG_REPO="$(mktemp -d)/catalog-skills"
mkdir -p "$CATALOG_REPO/skills/review-helper"
cat > "$CATALOG_REPO/skills/review-helper/SKILL.md" <<'EOF'
# Review Helper

Check behavioral regressions before style nits.
EOF

git -C "$CATALOG_REPO" init
git -C "$CATALOG_REPO" add .
git -C "$CATALOG_REPO" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -m initial

dalo source add-catalog public "$CATALOG_REPO"
dalo source inspect public
dalo source select public review-helper
dalo status
```

Review the pending skill, grant only that skill, then sync it:

```sh
dalo audit public:review-helper
dalo approve skill public:review-helper
dalo sync
```

`source add`, `source select`, and `approve skill` run deterministic local
preflight checks. `sync` repeats them against the exact content about to be
linked and blocks unaccepted `high` or `critical` findings from those checks or
a compatible cached review. `sync` does not start an agent reviewer, and a
passing preflight is not a safety guarantee.

An optional semantic review by an agent needs an installed Claude or OpenCode
CLI on `PATH` and may send the skill's contents to that provider, so it is not
part of the core flow:

```sh
dalo audit public:review-helper --reviewer auto
```

To accept a known risk for one exact content hash and finding set, give a
reason:

```sh
dalo audit public:review-helper --accept-risk "reviewed pinned upstream installer"
```

Changing any file, adding findings, or upgrading an audit or review engine
invalidates that acceptance. `dalo approve list` shows the local trust rules;
broader `source`, `author`, and `org` approvals exist when that is the intended
policy.

### Leaving the sandbox

```sh
unset DALO_STORE
```

The next command uses your real store again.

## Adopt a skill an agent wrote

When an agent creates a useful skill directly in its own folder, `status` lists
it under `unmanaged skills`. Copy it into the private local source:

```sh
dalo adopt release-notes
```

Adoption prints a local security preflight before it copies anything. Replacing
the original folder with a Dalo-owned symlink is a separate explicit step:

```sh
dalo adopt --replace release-notes
```

Dalo does not commit adopted work automatically. You decide when an experiment
is ready to move into the reviewed team repository.

## Where to go next

- [Team repository guide](team.md) — publish the source your teammates add
- [Command reference](reference.md) — every command, flag, and file format
- [Agent integration](agents.md) — supported agents and their directories
- [Troubleshooting](troubleshooting.md) — each diagnostic and the command that
  clears it
- [Dalo in CI](ci.md) — reproducible, non-interactive sync in a pipeline

# Getting Started

The recommended way to install and use Dalo is through your local coding
agent. For the terminal path in four commands, read the
[five-minute start](five-minutes.md). Paste this into the agent you want to set
up:

> Install Dalo, the agent skill manager.

The agent installs the binary and bundled assistant, connects its skill folder,
and checks the result. Then keep talking to it: “Add my team's skill repository,”
“Update my skills and show me what changed,” or “Check what needs attention.”
You do not need to learn Dalo's commands. Its approval, audit, and ownership
checks apply when your agent uses Dalo. See [Use Dalo through your agent](assistant.md)
for everyday requests, existing setups, and how those protections work.

The rest of this guide is the command-line walkthrough from a fresh install to
agents that read your team's skills. If you own the repository those skills
come from, read the [team repository guide](team.md) as well.

Before you start:

- install Dalo (see the [README installation instructions](../README.md#installation))
- have `git` on `PATH`
- use Linux or macOS

## Pick your starting point

You do not need a team repository or a clean machine to begin. Find the row
that matches your situation:

| Your situation | Start here |
| --- | --- |
| You want to look around without risk | [Explore without touching your agent folders](#explore-without-touching-your-agent-folders), then come back |
| Your team already has a skill repository | Continue with step 1 below |
| It is just you, with no team repository yet | Steps 1 and 2, then the catalog path in [step 3](#3-add-your-teams-skill-repository) |
| Skills already live in your agent folder | Steps 1 and 2, then [adopt them](#adopt-a-skill-an-agent-wrote) — Dalo leaves them untouched until you do |
| Your project installed skills with skills.sh | [Migrate a skills.sh project](projects.md#migrate-a-skillssh-project) |
| The skills belong to one repository, not your whole machine | [Project installations](projects.md) |

## 1. Let Dalo create the store

```sh
dalo
```

The first run creates `~/.dalo` and prints the summary with the next step:

```text
assistant: prepared the bundled Dalo skill in ~/.dalo/local/skills/dalo
assistant: next: dalo target detect
Dalo
  store: ~/.dalo
  initialized: yes
  linked targets: 0
  sources: 1
  active skills: 1
  pending approvals: 0

Next: dalo target detect
  Link a target so Dalo knows where to link skills.
```

The store is Dalo's local database: source checkouts, the private local source,
lockfiles, approvals, and target state. It lives in `~/.dalo` unless `--store`
or `DALO_STORE` says otherwise. Your agent folders are output, never input;
nothing in them is read back into the store by accident.

`dalo init` is the explicit form. Use it for a
[sandbox](#explore-without-touching-your-agent-folders) with its own store, or
to set up a [project](projects.md).

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
# or: dalo target link cursor
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
security preflight over every one of them before anything is linked. A clean
result takes one line:

```text
added source company (2 skills checked, no findings)
next: dalo sync
```

A finding, partial coverage, or a declared release binary prints the full audit
for that skill instead.

If your team repository also pins public catalogs in a `dalo.toml` manifest,
those arrive automatically as `company.<catalog-id>` sources. They are
untrusted, so their skills wait for your approval — see
[pending approval](troubleshooting.md#pending-approval).

No team repository yet? Skip `source add`. A public catalog gives you real
skills to start from, and you pick only the ones you want:

```sh
dalo source add-catalog sebastian https://github.com/sebastian-software/skills.sebastian-software.com.git
dalo source inspect sebastian
dalo source select sebastian effective-web
```

A selected catalog skill is untrusted until you approve it, which is the
[pending approval](troubleshooting.md#pending-approval) state. To publish a repository for
your team later, follow the [team repository guide](team.md).

## 4. Sync

```sh
dalo sync
```

`sync` refreshes clean tracking sources, resolves one approved skill set, and
links that set into every linked target:

```text
dalo store: ~/.dalo
target[claude]: ~/.claude/skills
existing noop       target[claude]:/dalo -> store:/local/skills/dalo
applied  create     target[claude]:/incident-review -> store:/sources/company/checkout/skills/incident-review
applied  create     target[claude]:/release-notes -> store:/sources/company/checkout/skills/release-notes
synced: 3 skills across 1 target (2 created, 1 unchanged)
security preflight: deterministic checks only
```

The last line qualifies the preflight that just ran on these new skills. A sync
that finds nothing new or changed leaves it out.

Dalo creates symlinks it owns. It never overwrites an unmanaged directory or a
symlink it did not create; anything it cannot do safely is reported instead.

Your agent now reads those skills from the folder it already uses.

## 5. Read the state

Two commands answer two different questions.

```sh
dalo status
dalo next
```

`status` is the full picture, and it opens with what needs you. The `needs you`
section lists every item that blocks or waits, each with the command that clears
it, and is absent when nothing does. Sources, targets, and active skills follow,
then the detail: pending approvals, unlinked skills, materialization blocks,
resolution diagnostics, and changes that are waiting for sync.
`dalo status --all` adds where each source comes from and the counts that are
zero. `dalo status --check` exits non-zero when anything needs attention, which
is what you want in a script or a pipeline.

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

When `status` shows something unexpected, it is usually pending approval,
shadowed, or conflict. The [three words status uses](troubleshooting.md#the-three-words-status-uses)
explains each one and the command that clears it.

## 6. Keep it current

Run `dalo sync` to fetch and apply updates from clean tracking team sources.
Personal catalog pins stay on their selected versions until you request an
update.

For recurring updates, ask your agent or host to automate this command for the
chosen store:

```sh
dalo --store <store-path> --json sync --check
```

Choose the cadence and notifications in that automation. `--check` still applies
the sync; it returns a failure when the result needs review. Pending approvals,
security findings, local edits, and conflicts require attention. See
[Automate updates](assistant.md#automate-updates) for the scope and update-policy
choices. Existing [autosync installations](reference.md#dalo-autosync-installstatusuninstall)
remain supported in Dalo 1.x.

For a focused health check:

```sh
dalo doctor
```

## Explore without touching your agent folders

Everything above also works in a throwaway store with a temporary folder as the
target. Nothing in this section writes to a real agent directory. A sandbox
store stays quiet about the Dalo skill that your global store already delivered
to your agent folders; `dalo assistant status` and `dalo doctor`
(`assistant_external`) still report it on request.

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
---
name: review
description: Check behavioral regressions before style nits.
---

# Review

Check behavioral regressions before style nits.
EOF

dalo status
dalo sync
ls -la "$target_dir"
```

The local source is private to your machine: experiments and overrides live
there. Until the next `sync` records the new skill, `status` lists it as waiting:

```text
needs you:
  review (local): waiting for sync
    run: dalo sync
```

See [Lock Drift](troubleshooting.md#lock-drift) for what else status reports
before a sync.

### A local team source

A local Git repository works as a source, so this needs no network access:

```sh
TEAM_REPO="$(mktemp -d)/team-skills"
mkdir -p "$TEAM_REPO/skills/release-notes"
cat > "$TEAM_REPO/skills/release-notes/SKILL.md" <<'EOF'
---
name: release-notes
description: Summarize user-visible changes first.
---

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
[pending approval](troubleshooting.md#pending-approval) comes from:

```sh
CATALOG_REPO="$(mktemp -d)/catalog-skills"
mkdir -p "$CATALOG_REPO/skills/review-helper"
cat > "$CATALOG_REPO/skills/review-helper/SKILL.md" <<'EOF'
---
name: review-helper
description: Check behavioral regressions before style nits.
---

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
preflight checks and report a clean result in one line, such as
`selected review-helper from public (checked, no findings)`; `dalo audit`
always prints the full report. `sync` repeats the checks against the exact
content about to be
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

Adoption runs a local security preflight before it copies anything. A clean
result shows as `(checked, no findings)` on the copy line. A finding prints the
full audit, and a high or critical one stops the adoption until you accept the
risk. Replacing the original folder with a Dalo-owned symlink is a separate
explicit step:

```sh
dalo adopt --replace release-notes
```

Dalo does not commit adopted work automatically. You decide when an experiment
is ready to move into the reviewed team repository. When it is, `promote` opens
a pull request against that repository instead of pushing to it:

```sh
dalo --dry-run promote release-notes --target company
dalo promote release-notes --target company
```

`company` is the ID of the team source. The real run needs an authenticated
GitHub CLI (`gh`); see [`dalo promote`](reference.md#dalo-promote) for forks and
the cases it blocks.

## Where to go next

- [Team repository guide](team.md) — publish the source your teammates add
- [Project installations](projects.md) — pin skills per repository and restore
  them with `dalo install`
- [Dalo assistant](assistant.md) — do all of this by talking to your agent
- [Command reference](reference.md) — every command, flag, and file format
- [Agent integration](agents.md) — supported agents and their directories
- [Troubleshooting](troubleshooting.md) — each diagnostic and the command that
  clears it
- [Dalo in CI](ci.md) — reproducible, non-interactive sync in a pipeline
- [Security overview](security.md) — what the preflight and approvals do and
  do not protect against

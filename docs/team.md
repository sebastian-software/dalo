# Team Repository Guide

This guide is for the person who owns the repository a team syncs from. It goes
from an empty Git repository to a manifest that pins exactly which skills every
teammate resolves, and it ends with the two commands a teammate runs on a new
laptop.

The developer side is the [getting started guide](getting-started.md). Command
syntax and file formats are in the [command reference](reference.md).

## What a team repository is

A team repository is an ordinary Git repository with two things in it:

- `skills/<name>/SKILL.md` — the skills your team wrote
- `dalo.toml` — an optional manifest that pins external catalogs and selects a
  subset of their skills

Teammates add it once with `dalo source add <id> <git-url>`. Everything else —
which external skills exist, at which commit, in which order — travels in the
repository and moves through code review like any other change.

Two things deliberately do not travel: security approvals and the target
folders each teammate links. Those stay personal.

## 1. Create the repository

Create the Git repository first, then write the manifest from inside it:

```sh
dalo team init company --name "Company Skills"
```

```text
initialized team manifest …/company-skills/dalo.toml
next: commit and push dalo.toml
```

`company` is the source ID teammates will see locally. Run `team` commands from
the checkout, or point at another one with `dalo team --repo ../company-skills
init company`.

Team commands only edit the team repository. They never read or initialize a
personal Dalo store, never grant approvals, and never commit or push. The
global `--store` flag is accepted for uniformity but has no effect here.

## 2. Add the team's own skills

Nothing special is required. Each skill is a directory under `skills/` with a
`SKILL.md`:

```sh
mkdir -p skills/release-notes
cat > skills/release-notes/SKILL.md <<'EOF'
# Release Notes

Summarize user-visible changes first.
EOF

git add skills/release-notes
git commit -m "feat: add the release-notes skill"
```

Skills in a team source are trusted by default, so a teammate's `sync` links
them without a per-skill approval. Sync still audits their content and blocks
unaccepted high or critical findings.

## 3. Pin a public catalog

External skill sets are added as pinned catalogs. Pin a commit, and select only
what the team should actually get:

```sh
dalo team catalog add marketing https://github.com/coreyhaines31/marketingskills.git \
  --version 0123456789abcdef0123456789abcdef01234567 \
  --skill +copywriting \
  --skill +launch
```

```text
catalog_added team manifest …/company-skills/dalo.toml catalog=marketing
next: commit and push dalo.toml
```

`--version` accepts a commit, tag, or ref. An immutable commit is the
reproducible choice; a branch name means different teammates can resolve
different content. Use `--priority <number>` to override the derived default.

The manifest now reads:

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
skills = [
    "+copywriting",
    "+launch",
]
```

### Filters

`dalo team catalog skills <id> [filter]...` replaces the whole filter list:

```sh
dalo team catalog skills marketing +copywriting +launch -seo-audit
dalo team catalog skills marketing
```

The second form sets `skills = []`, which means everything. Evaluation is
set-based and independent of order:

- omitted or empty: every skill in the catalog
- only `-name` entries: everything except those
- any `+name` entry: whitelist mode
- exclusions always win over inclusions
- a bare name counts as an include

Unknown or ambiguous filter references block a teammate's sync, so keep the
filters in step with the pinned commit.

Check what the manifest says at any time:

```sh
dalo team show
```

```text
team manifest: …/company-skills/dalo.toml
source: company (Company Skills)
catalogs:
  marketing version=28a2eec94f0e18ba5582792bbeb82a25bffedf02 skills=+copywriting, +launch …
```

## 4. Advance a pin on purpose

Upstream moves. Preview first:

```sh
dalo --dry-run team catalog update marketing --from main
```

```text
team catalog marketing: de993fb5ea15 -> 28a2eec94f0e (from main)
  inventory:
    selected_changed `copywriting` changed upstream
  audits:
    company.marketing:copywriting clean
    company.marketing:launch clean
  result: would update (…/company-skills/dalo.toml)
```

Dalo resolves the ref in a temporary clone, compares the declared version with
the candidate inventory, and runs deterministic audits over the selected
candidate skills. A dry run performs network reads and temporary filesystem
work; it changes neither the repository nor any personal store.

When the report looks right, run it for real:

```sh
dalo team catalog update marketing --from main
```

```text
  result: updated (…/company-skills/dalo.toml)
next: commit and push dalo.toml
```

The write is always an exact commit — `--from main` never leaves a floating ref
in the manifest. A non-fast-forward candidate, a selected skill that disappeared
upstream, an invalid selection, or a blocking audit finding prevents the write.
`--accept-risk "<reason>"` accepts blocking security findings from that exact
candidate only; it does not override the structural blockers.

To set a version without consulting upstream, use
`dalo team catalog version marketing v2.0.0`. To drop a catalog entirely, use
`dalo team catalog remove marketing`; the next teammate sync removes that
catalog's generated source state, approvals, owned links, and checkout.

Changing a catalog's URL is a deliberate two-step replacement: remove the
declaration and sync, then add the reviewed replacement URL and sync again.

## 5. Commit and push

Dalo never commits for you. The manifest is a reviewable file like any other:

```sh
git add dalo.toml
git commit -m "chore: advance the marketing catalog pin"
git push
```

Catalog mutations rewrite `dalo.toml` in canonical TOML form. Formatting may be
normalized and comments are not preserved, so keep the rationale in the commit
message or the pull request.

## 6. What a teammate runs

On their own machine, once:

```sh
dalo init
dalo target detect
dalo target link claude
dalo source add company git@github.com:acme/agent-skills.git
dalo sync
```

Your team skills link immediately. The catalogs you pinned show up as their own
source, namespaced with the team ID:

```text
sources:
  company      team priority=10   skills=1   agents=0   plugins=0   enabled
  company.marketing catalog priority=11   skills=3   agents=0   plugins=0   enabled
    provenance management=team_manifest origin=… requested=28a2eec94f0e… resolved=28a2eec94f0e
```

Their URL, version, priority, and selection are owned by your manifest.
`source select`, `source priority`, `source remove`, and pin advancement are
rejected for these derived catalogs on a teammate's machine — the manifest is
the single place they change.

## 7. Approvals stay personal

Manifest-derived catalogs are untrusted, so the first sync links your team's own
skills and leaves the catalog skills pending:

```text
synced: 1 skill across 1 target (1 created)
pending approval: company.marketing:copywriting (run: dalo approve skill company.marketing:copywriting)
pending approval: company.marketing:launch (run: dalo approve skill company.marketing:launch)
```

Each teammate reviews and approves on their own machine:

```sh
dalo audit company.marketing:copywriting
dalo approve skill company.marketing:copywriting
dalo approve skill company.marketing:launch
dalo sync
```

Approvals are bound to exact content, so advancing a pin makes the changed
skills pending again. That is the intended behavior: a commit you push cannot
silently put new third-party code into a teammate's agent. If your team prefers
a broader policy, `dalo approve list` and the `source`, `author`, and `org`
approval scopes exist — they are still granted per machine.

## 8. Keeping machines current

Tell teammates to install the scheduler once:

```sh
dalo autosync install --schedule daily
dalo autosync status
```

macOS uses launchd, Linux a systemd user timer with a marked crontab fallback.
A scheduled run is a `dalo sync --check`: it refreshes clean sources and links
what is already approved, and it stays fail-closed on dirty sources, pending
approvals, security findings, and target conflicts. It never grants an approval.
The last attempt, the last success, and any blocking reason stay visible in
`dalo status` and `dalo doctor`.

So the rhythm after you push a manifest change is: autosync picks it up, and a
teammate sees either new skills or an exact `dalo approve skill …` line.

## 9. A second machine

Same team, new laptop. Nothing is copied from the old machine:

```sh
dalo init
dalo target detect
dalo target link claude
dalo source add company git@github.com:acme/agent-skills.git
dalo sync
dalo next
```

`dalo next` names the one remaining command, which on a fresh machine is
usually the first pending approval. Approve, sync again, and the two machines
resolve the same skill set from the same manifest.

Approvals do not transfer, and that is deliberate. Personal local skills under
`local/skills/` do not transfer either; move anything worth keeping into the
team repository with `dalo adopt` and a commit.

## Checklist

- [ ] `dalo team init <id> --name "<name>"` in the repository
- [ ] team skills under `skills/<name>/SKILL.md`
- [ ] each external catalog added with `--version <commit>` and explicit filters
- [ ] pins advanced through `dalo --dry-run team catalog update <id> --from <ref>` first
- [ ] `dalo.toml` committed and pushed
- [ ] teammates told the one `dalo source add <id> <git-url>` line
- [ ] teammates know approvals are theirs to grant

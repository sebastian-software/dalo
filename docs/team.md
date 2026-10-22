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

When a repository contains unrelated files or history, add `--path <directory>`
to acquire just the directory containing the selected skills. This requires a
full lowercase commit ID and sets `schema_version = 2` in the manifest. Sync
uses a shallow, blob-filtered sparse checkout; the Git server must support
fetching the exact commit and filtering. Existing clean catalog checkouts are
replaced after the new content passes audit. If a checkout has local files or
changes, sync stops so they can be reviewed before retrying.

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
  pin: de993fb5ea15… -> 28a2eec94f0e18ba5582792bbeb82a25bffedf02
  inventory:
    selected_changed `copywriting` changed upstream
  audits:
    company.marketing:copywriting clean (0 findings, …)
    company.marketing:launch clean (0 findings, …)
  result: would update (dalo.toml)
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
  result: updated (dalo.toml)
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

For review in GitHub, start from a clean team checkout at the current default
branch and use the explicit PR mode instead:

```sh
dalo --dry-run team catalog update marketing --from main --pr
dalo team catalog update marketing --from main --pr
```

Dalo uses the authenticated GitHub CLI and repository write access to commit
only the pin change in a temporary checkout, push a proposal branch, and open a
PR. Your checkout remains unchanged. The PR body is the same text as the human
dry-run output, with full pins, drift and audit findings. Retry the same command
after a PR creation failure to resume the pushed branch; an existing open PR is
reused. Changed proposal branches and closed PRs require manual review. This
mode supports GitHub.com origins.

### Follow a first-party branch on each sync

For a catalog you maintain and want to follow continuously, opt in explicitly:

```sh
dalo team catalog add internal https://github.com/acme/internal-skills.git \
  --version main --track
```

For an existing catalog, use `dalo team catalog version internal main --track`.
These commands upgrade the manifest to schema version 3:

```toml
schema_version = 3

[source]
id = "company"

[[catalog]]
id = "internal"
url = "https://github.com/acme/internal-skills.git"
version = "main"
update = "track"
```

Each real `dalo sync` fetches that exact branch and accepts only fast-forward
updates after validating the selection and auditing its contents. The local
source lock records the exact installed commit. Other catalogs keep their pins,
including declarations that say `version = "main"` without the explicit policy.
Consumers can resolve different commits at different times; use reviewed pins
when every machine must resolve the same revision.

Tracking does not grant trust or execution approvals. Existing skill approvals
match the source-qualified skill identity and remain valid across content
updates. An approved skill therefore receives audited updates automatically.
New skill identities remain pending unless an existing broader local approval
covers them. Agents, tools, and hooks retain their own approval gates; audit
risk acceptances remain bound to their reviewed content and scope.

A failed fetch, missing branch, non-fast-forward update, removed selected skill,
blocking audit, or local checkout content blocks reconciliation and preserves
the previous catalog pins and deliveries. Dalo does not silently proceed using
stale upstream information. A later reconciliation failure also rolls back
catalogs already advanced in that pass. As with pinned manifests, this is a
rollback boundary within a running sync, not a crash-recovery journal.

`sync --dry-run` does not fetch. It reports the existing local commit and marks
upstream as not checked; it cannot predict the next remote commit. Sync's human
and JSON reports include each catalog's policy, requested ref, previous commit,
resulting commit, and whether upstream was checked.

Branch tracking requires a full catalog checkout. It cannot be combined with
`path`, whose shallow sparse checkout lacks the ancestry needed to verify a
fast-forward. A branch rewrite requires explicit review: use
`dalo team catalog version internal <exact-commit>` to return to a fixed pin.
The same command without `--track` always disables tracking. Upgrade consumers
to a Dalo version supporting team schema 3 before publishing the manifest.

## 5. Commit and push

In the default manual flow, commit and push the reviewable manifest yourself:

```sh
git add dalo.toml
git commit -m "chore: advance the marketing catalog pin"
git push
```

Manual catalog mutations rewrite `dalo.toml` in canonical TOML form. Formatting may be
normalized and comments are not preserved, so keep the rationale in the commit
message or the pull request. PR mode preserves existing comments and formatting
and changes only the reviewed version value.

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
source, namespaced with the team ID, and `dalo status --all` adds where each
one comes from:

```text
$ dalo status --all
…
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

Skill approvals match source-qualified identities and remain valid when those
skills change. New identities need a matching local approval, while every
selected skill still passes security audits. Accepted audit risks are bound to
the exact reviewed content and scope. Use reviewed commit pins when changes
must pass team review before reaching consumer machines. Broader `source`,
`author`, and `org` approvals are also local choices; inspect them with
`dalo approve list`.

## 8. Keeping machines current

After publishing a team change, teammates run `dalo sync` to refresh their
tracking team source and reconcile the declared catalogs. For recurring updates,
each teammate can ask their agent or host to automate the chosen store:

```sh
dalo --store <store-path> --json sync --check
```

The automation chooses when to run, when to retry, and how to notify its owner.
`sync --check` still applies eligible changes and returns a failure for an
incomplete result. It refreshes tracking sources and follows the team's declared
catalog pins or explicitly tracked catalog branches, while personal catalog
pins stay unchanged. It never grants an approval or bypasses dirty sources,
blocking audits, or target conflicts.

Read the JSON report even on a nonzero exit; safe work may have completed while
other work remains blocked. The agent can explain changed skills or the exact
approval needed and notify the teammate through its own host. See
[Automate updates](assistant.md#automate-updates). Existing
[autosync installations](reference.md#dalo-autosync-installstatusuninstall) remain
supported in Dalo 1.x.

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
`local/skills/` do not transfer either; send anything worth keeping to the team
repository with `dalo promote`, described next.

## 10. Take contributions from teammates

A skill that started as one person's experiment reaches the team through a pull
request, not a push. From their own machine, a teammate previews and then
submits one local skill:

```sh
dalo --dry-run promote review-helper --target company
dalo promote review-helper --target company
```

Dalo audits the skill, commits only that skill on a fresh `dalo/promote-*`
branch in a temporary clone, and opens the pull request against the
repository's configured or default branch. It never pushes to that branch, and
an unaccepted blocking audit finding stops the promotion. The real run needs an
authenticated GitHub CLI (`gh`) and works with GitHub.com repositories; a
teammate without push access adds `--fork`. You review and merge it like any
other change. Details and the blocked cases are in the
[`dalo promote` reference](reference.md#dalo-promote).

## Checklist

- [ ] `dalo team init <id> --name "<name>"` in the repository
- [ ] team skills under `skills/<name>/SKILL.md`
- [ ] each external catalog added with `--version <commit>` and explicit filters
- [ ] pins advanced through `dalo --dry-run team catalog update <id> --from <ref>` first
- [ ] `dalo.toml` committed and pushed
- [ ] teammates told the one `dalo source add <id> <git-url>` line
- [ ] teammates know approvals are theirs to grant
- [ ] teammates know `dalo promote <skill> --target <id>` proposes a local skill
      as a pull request

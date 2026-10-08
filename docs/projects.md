# Project installations

Dalo restores a project's skills from a checked-in definition into an independent
store. Inside a configured project, including its subdirectories, use:

```sh
dalo install
dalo status
dalo doctor --check
```

Dalo searches upward for the nearest `dalo-project.toml`, stopping at the nearest
Git repository boundary (including worktrees and submodules). Without a Git
boundary it searches up to the filesystem root. A malformed or redirected
definition fails instead of silently selecting the global store.

## Choosing scope

| Selection | Behavior |
| --- | --- |
| `--global`, `-g` | Use `~/.dalo`, bypassing project discovery and `DALO_STORE`. |
| `--project <directory>` | Use that exact project directory, without parent discovery. |
| `--store <path>` | Use an explicitly chosen store, bypassing project discovery. |
| `DALO_STORE` | Preserve an explicitly configured environment store, bypassing discovery. |
| No override, project definition found | Use the nearest project's `.dalo` store. |
| No override, no project definition | Preserve the global default, `~/.dalo`. |

The three CLI scope flags conflict with one another. Team authoring, completions,
manpages, and standalone plugin validation do not acquire an automatic project
scope. Unsupported project commands fail rather than operating globally.

To set up a project, run `dalo init` interactively inside its Git repository.
If no definition or store override exists, Dalo asks whether to initialize the
project (at the Git root) or the global store. Enter, EOF, or an unknown answer
cancels without writing. JSON, dry-run, CI, and non-interactive invocations never
prompt; without a definition they retain the existing global behavior. Choose
explicitly in scripts:

```sh
dalo --project . init  # Create this project's definition.
dalo init --global    # Initialize the home store.
```

Human project output identifies the scope. Existing JSON status and sync reports
retain their schemas and identify the project store through their store path.

## Definition and local state

Commit `dalo-project.toml`. It is a separate, versioned format from the
existing team-source `dalo.toml`; both can coexist in a repository.

```toml
schema_version = 1
targets = ["claude", "codex"]

[[source]]
id = "team"
url = "https://github.com/example/team-skills.git"
# Replace with the complete lowercase commit ID from this repository.
commit = "0123456789abcdef0123456789abcdef01234567"
skills = ["review", "documentation"]
```

Sources use explicit catalog skill selectors (name, stable ID, or relative
skill path). Dependencies use the existing resolver. Sources earlier in the
list take precedence over later sources. Supported targets are `claude`,
`codex`, `opencode`, `hermes`, and `openclaw`; Codex and OpenClaw share
`.agents/skills`, so choose only one of those target IDs.

Schema version 2 adds an optional top-level `approval` field that chooses who
approves the selected skills: `"local"` (the default, as in schema version 1)
or `"declaration"`. See [Approval modes](#approval-modes).

`init` creates the definition only and never replaces an existing one; it
writes schema version 1. `install` creates `.dalo/` inside the project, clones
the declared sources at their exact commits, and links approved skills into the
selected agents' project folders. Each fresh clone or worktree gets its own
store. The global store and global agent folders are not part of this
installation.

Add the following entries to your project's `.gitignore`, selecting only the
agent folders managed by this definition:

```gitignore
/.dalo/
/.claude/skills/
/.agents/skills/
```

Dalo does not edit `.gitignore` or take over existing skill directories. Keep
project-authored skills that are already versioned; a same-name unmanaged entry
blocks delivery instead of being replaced. Generated links are absolute paths
into the local store and must not be committed. The store's machine-specific
locks and approvals must not be committed either.

## Review and installation

By default the definition declares desired content, not approval. Initial
installation prepares the pinned sources but exits nonzero when approval is
pending. Review and approve skills locally, then rerun installation:

```sh
dalo status
dalo audit team:review
dalo approve skill team:review
dalo install
dalo doctor --check
```

Each clone and worktree has its own store, so each one repeats these local
approvals. A project that reviews its declaration in pull requests can instead
make the declaration the approval authority; see [Approval modes](#approval-modes).

Security findings still require the existing explicit, content-bound review
flow in both modes. Installation never creates approval records or executes
skill contents. Repeated installation preserves the pins even if upstream has
advanced. Dirty checkouts, changed pins, redirected output directories, and
unmanaged same-name entries are not overwritten. After a failed initial
preparation, registered sources remain available for inspection and a retry; an
unregistered leftover checkout is reported for manual preservation/recovery.

`--dry-run install` validates and previews the declaration without fetching,
creating a store, auditing remote content, or delivering links. It is not a
promise that the remote sources or approvals are ready.

## Approval modes

Human `install` and `status` output names the active mode on stderr, for example
`approvals: declaration (dalo-project.toml)`. The `--dry-run --json install`
preview reports it as `approval`.

| `dalo-project.toml` | Who approves | Fresh clone or worktree |
| --- | --- | --- |
| `schema_version = 1` | Each project store, in `.dalo/approvals.toml` | Pending until approved locally |
| `schema_version = 2` without `approval`, or `approval = "local"` | Each project store, in `.dalo/approvals.toml` | Pending until approved locally |
| `schema_version = 2` and `approval = "declaration"` | The reviewed declaration | Installs the declared selection directly |

With `approval = "declaration"`, every explicitly selected skill and the
required skills it pulls in are approved for that project store without local
approval records. A fresh clone, a new worktree, a `postinstall` hook, or CI
restores the same reviewed skill set with one `dalo install`:

```toml
schema_version = 2
approval = "declaration"
targets = ["claude", "codex"]

[[source]]
id = "team"
url = "https://github.com/example/team-skills.git"
commit = "0123456789abcdef0123456789abcdef01234567"
skills = ["review", "documentation"]
```

The approval comes from repository trust. A repository could already commit
the same skill content into `.claude/skills/` or run code from a `postinstall`
script, and a full-commit pin is equivalent to committing that content. Opt in
only when changes to `dalo-project.toml` get the same review as dependency
changes. Read a new pin, selection, or mode like a dependency bump: inspect the
skills it brings in, not only the commit line. See
[Security](security.md#trust-boundaries) for what this mode does not protect
against.

The opt-in changes approval only. Everything else stays as it is:

- Deterministic audits run on every install, and unaccepted high or critical
  findings still block delivery. An accepted risk stays local and bound to the
  exact audited content. Record it with
  `dalo audit <source:skill> --accept-risk "<reason>"`; that command does not
  create an approval record.
- Only the explicit selection and its required closure are delivered. Other
  skills in the same source stay unselected offers.
- Dirty sources, changed pins, redirected paths, unmanaged same-name entries,
  and a foreign `.dalo` still block installation.
- Project scope delivers skills only. A project installation must not change
  the global store, global agent folders, or provider settings; such an effect
  is a bug, not something the declaration can approve.

There are no local opt-outs. To deliver a different set, change the declaration
through the repository's review.

### Opt in an existing project

1. Make sure every teammate, CI job, and automation uses a Dalo version that
   supports project schema version 2. Dalo 1.4.0 and earlier reject the file
   instead of guessing.
2. Set `schema_version = 2`, add `approval = "declaration"`, and review that
   change like any other declaration change. `project add`, `project update`,
   and `project remove` keep both lines and their comments.
3. Run `dalo install` after pulling it. Until then, `status`, `doctor`,
   `audit`, `approve`, `project add`, and `project update` ask for this install
   because the store does not match the declared mode yet.

The first install after opting in activates every declared skill that was
still pending and lists each one as an ordinary `create` sync operation, so the
output shows exactly what the opt-in delivered. Existing records in
`.dalo/approvals.toml` are kept but not needed while the mode is `declaration`.
Install neither creates nor deletes them for the mode switch. A later pin update
still revokes local records for changed or removed content, so they can never
approve stale content.

To return to local approval, set `approval = "local"` (or remove the field) and
run `dalo install`. Skills without a matching local approval return to pending
and their links are removed; preserved local approvals apply again.

## Add a source from the project

Use `dalo project add` from inside the project (or pass `--project <directory>`)
to resolve a repository and explicitly selected skills into the portable
definition. It uses the project scope discovered from `dalo-project.toml`; it
does not add a global source. The first call is a read-only preview:

```sh
dalo project add company https://github.com/acme/skills.git --ref main --skill review
```

The preview fetches the repository into a temporary directory and shows the
resolved full commit, selected skills, project target folders, exact TOML entry,
and the command to apply it. It does not create `.dalo`, approve skills, or link
content. Review the source and selection before writing the declaration.

For a branch or tag, apply only if the ref still resolves to the commit you
reviewed. Copy the full commit ID from the preview into `--expect-commit`:

```sh
dalo project add company https://github.com/acme/skills.git --ref main \
  --skill review --expect-commit <previewed-commit> --apply
dalo install
```

An immutable full commit can be added directly with `--ref <full-commit> --apply`.
Each `--skill` is required; selectors may be a catalog name, stable ID, or skill
path. Dalo stores stable IDs where available and paths otherwise. Local Git
repositories are allowed only when their path is inside the project, so the
committed URL stays portable. Duplicate source IDs are blocked; reviewed changes
to an existing source or selection belong to the separate update workflow.

Adding a source edits only `dalo-project.toml`. Run `dalo install` afterwards to
prepare the pinned checkout. With local approval it uses the ordinary project
review and approval flow, and does not activate unapproved skills. A teammate can
commit the declaration and restore the same commit and selection with
`dalo install` in a fresh checkout; their approvals remain local. With
`approval = "declaration"`, the reviewed declaration change is the approval:
install activates the new selection and its required skills on every checkout,
subject to the same audits.

## Update a project source

Use `dalo project update` to review a new revision for an existing source. The
default is read-only and compares the selected skills, dependency declarations,
and candidate audits with the current pin:

```sh
dalo project update company --ref main
dalo project update company --ref main --expect-commit <previewed-commit> --apply
dalo install
```

Applying a branch or tag requires the exact `--expect-commit` from the preview.
An immutable full commit can be applied directly. If no `--skill` options are
given, Dalo preserves the current selection by stable ID (or path when a skill
has no stable ID). Repeat `--skill` to replace that selection explicitly.
Removed selected skills block an implicit update; provide a replacement
selection to proceed. Dalo edits only the source's `commit` and `skills` values
in `dalo-project.toml`, preserving surrounding comments and formatting.

The declaration change and local installation are separate steps. `dalo
install` stages a new commit-specific checkout without overwriting the old one,
then reconciles delivery through the normal transactional sync. Dirty source
checkouts and unmanaged target content block the operation. Per-skill approvals
for changed or removed content are revoked in the local project store, including
required dependencies and previously deselected skills. Unchanged content keeps
its decision. With local approval, changed skills stay inactive until their new
content is reviewed and approved locally. With `approval = "declaration"`, the
reviewed update is the approval, and install activates the changed content
directly. Accepted audit risks remain
bound to the exact audit content hash. Candidate audit findings are shown during
preview and remain blocking until resolved; a blocking finding also prevents
applying the declaration update.
If config and source-lock updates are interrupted, the next `dalo install`
restores their recovery snapshot and retries. Old checkouts remain available
after a successful update so a failed delivery can be retried without losing
the prior links.

## Remove project sources or skills

Preview removing an entire source or exact selectors from its declaration:

```sh
dalo project remove company --skill review
dalo project remove company --skill review --apply
dalo --dry-run --json install
dalo install
# To stop delivery from the whole source:
dalo project remove company --apply
dalo install
```

Without `--apply`, removal only previews the declaration and local delivery
effects. `--dry-run` suppresses applying even when `--apply` is present. Repeat
`--skill` to remove several exact selectors from the declaration's `skills`
array. Removing its final selector removes the source entry. A required skill
stays selected through any remaining consumer; remove those consumers to stop
its delivery. An implicit dependency is not an independently removable selector.

`--apply` edits only `dalo-project.toml`, preserving unrelated entries and
comments. Commit that change for teammates. `install` reconciles the declaration
on an existing clone, including changes pulled from Git; a fresh clone installs
the same remaining selection. Verified Dalo-owned links are removed, and foreign
symlinks or real directories at former link paths are preserved. Unrelated
sources, targets, and the global store remain independent.

Removing a whole source unregisters its local pin and revokes its source-scoped
approvals. Deselection alone retains approvals for unchanged content. Cached
checkouts are **always retained**, including dirty checkouts and older pins;
removal stops delivery and does not delete local work. Dirty sources that remain
declared still block installation. Redirected paths or inconsistent pins also
block removal. Preserve local work before cleaning up retained caches manually;
there is no automatic project cache cleanup. A retained checkout with local edits
cannot be overwritten by re-adding the same source ID.

The preview shows deactivated skills (including unused dependencies), planned
link actions, approvals to revoke, and retained checkouts with their dirty state.
It uses installed pins without fetching; unavailable or changed declared pins
set `delivery_preview_complete` to false. `--dry-run --json install` includes
these local effects in `removals`. Ordinary install reports the actual sync
actions. Interrupted metadata updates use the existing project recovery journal;
rerun `dalo install` to recover and retry. A failed delivery can also be retried
without resetting `.dalo` or deleting approvals.

## Project command boundary

Project scope supports `project add`, `project update`, `project remove`, `init`, `install`,
`status`, `doctor`, `audit`, and `approve`. Other project-scoped commands fail
instead of falling back to the global store. Use `install` to reconcile
delivery; `sync` is not yet supported in project scope. Use `dalo sync --global`
when you intend to synchronize the global store from inside a project.

Project declarations keep immutable full commit IDs as the reproducibility
boundary, so no separate portable lockfile is needed. Changing a source's URL
under the same ID remains explicit migration work. Declaration removals are
reconciled by install; they never authorize deleting cached content.

Project `install`, `status`, and `doctor` never read or modify global provider
hook configuration, such as `~/.claude/settings.json` or `~/.codex/hooks.json`.
Hooks projected by the global store cannot block a project installation, and
project reports list no hook targets.

## Migrate a skills.sh project

For a project with a version 1 `skills-lock.json`, preview a verified migration:

```sh
dalo migrate skills-sh
# Review every result and the inferred agent targets, then apply:
dalo migrate skills-sh --apply
dalo install
# Review pending skills and approve each intended selector, for example:
dalo approve skill imported-1:review
dalo install
```

The generated definition uses schema version 1 with local approval. To let the
reviewed definition approve its selection instead, opt in as described in
[Approval modes](#approval-modes) before committing it.

The command searches upward for `skills-lock.json`, stopping at the nearest Git
boundary. `--project <directory>` selects an exact directory. Global/store
selection is rejected. `--json` returns the verification report; a blocked entry
causes a nonzero exit. `--dry-run` suppresses `--apply`.

Verification clones Git sources into temporary directories, even for previews
and dry-runs. It does not execute skill code. The skills CLI's `computedHash` is a
content hash, **not a Git commit**. Dalo checks the source's recorded `ref`, or its
current default branch when none is recorded, and compares every installed copy
with that candidate. Only identical file contents, paths, and executable bits
qualify. The generated definition pins the verified commit; this does not claim
to recover the original historical revision. An upstream change or a local edit
blocks migration instead of silently upgrading or discarding content.

This initial importer supports Git origins, existing copies in `.agents/skills`,
`.claude/skills`, `.opencode/skills`, and `.hermes/skills`, and aliases between
those project folders. `.agents/skills` maps to the Codex target (also shared by
OpenClaw). All imported skills are selected for all inferred targets. Review this
union in the preview. Nested symlinks, external links, unknown source types,
missing copies, ambiguous identities, and unsupported lock versions require
manual migration. Global skills locks, subagent records, and other agent folders
are not imported. Unlisted personal skills remain untouched.

If any entry is blocked, **nothing is applied**. Preserve edited or unrecognized
content separately and resolve its ownership before retrying. An existing Dalo
project definition, project store, or migration backup also blocks this first
handover; the command does not merge definitions.

Applying creates `dalo-project.toml` and moves the verified old skill directories,
aliases, and `skills-lock.json` into `.dalo-migration-backup/`, retaining their
original relative paths. It does not import trust approvals or install the new
links. Skills become active again after the normal install/approval steps above.
Add `/.dalo-migration-backup/` to `.gitignore`, alongside the project store and
managed agent folders. Review the Git diff: commit the definition and intentional
removal of previously checked-in skills, not the backup or generated store.

Keep the backup until the new installation works. `migration.json` records the
pre-apply plan for recovery, not a completion receipt. After an interrupted apply,
inspect both locations: restore backed-up entries to their original relative
paths only when those destinations are empty. Never overwrite a newly created
file or link. Backed-up relative symlinks may be dangling inside the backup; their
original link text is retained for restoration. A normal apply error attempts to
restore moved entries, while retaining the backup directory for inspection.

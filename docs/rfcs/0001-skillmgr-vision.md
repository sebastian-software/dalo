# RFC 0001: Skillmgr Vision

Status: Draft  
Date: 2026-06-23  
Author: Sebastian + Codex  

## 1. Summary

Skillmgr is a CLI-first manager for AI-agent skills on macOS and Linux. It centralizes skills and instruction packs from multiple Git-based sources, resolves them into one final asset set, materializes skills into configured agent skill directories through symlinks, and renders instruction packs into managed agent-instruction blocks.

The vision addresses six core problems:

1. Teams need shared, versioned skills that can update automatically.
2. Users often create and refine skills directly with agents inside existing agent skill directories.
3. Individual users need private skills that do not automatically belong to the team.
4. A user may belong to multiple teams or skill sources, and the final available asset set should be the sum of all active sources.
5. Useful third-party repositories often contain many skills, of which a team may only want to use a selected subset.
6. Teams and individuals also need lightweight shared conventions that belong in agent instruction files rather than in executable tool configuration.

Skillmgr is intentionally not a public registry service and not a project-profile switcher. The central product idea is a robust source resolver with a local store, safe sync, visible conflicts, adoption of existing skills, and PR-first promotion into team repositories.

## 2. Problem Statement

Current skill-management tools are typically optimized for individual users and one-off installation flows. Team workflows need capabilities that are usually missing:

- central versioning for team skills
- reproducible updates instead of uncontrolled "latest"
- clear separation between team skills, private user skills, and unmanaged agent skills
- multi-agent installation without repeated manual choices
- multi-team resolution with visible conflicts
- selective use of multi-skill third-party repositories
- visibility into upstream repository structure changes, new skills, removed skills, and moved skills
- dependency awareness when selected skills depend on other skills from the same upstream repository
- versioned team and user instruction packs for standing agent-facing conventions
- a simple feedback path from locally developed skills into a team repository
- automatic but safe sync

One obvious approach would be to make the agent skill directory itself a Git repository. This RFC rejects that approach as the default model because the agent directory would then become the installation target, active working tree, sync target, local experimentation area, and team database at the same time. That creates fragile merge situations in the exact directory that agents actively read and write.

Instead, skillmgr manages its own store and uses agent directories as materialization targets and discovery locations.

## 3. Goals

- Manage team skills from Git repositories and keep them updated regularly.
- Support multiple active skill sources at the same time.
- Resolve source priorities deterministically and with user configuration.
- Materialize skills into multiple agent target directories through symlinks.
- Detect existing unmanaged skills without changing them implicitly.
- Make local skills versionable in a private local Git repository.
- Promote local skills or dirty team edits into team sources through a PR-first flow.
- Pin external skill sources reproducibly through manifests and lockfiles.
- Select individual skills from multi-skill external repositories without vendoring the whole repository.
- Detect upstream inventory drift for selected external repositories, including new skills, removed selected skills, moved selected skills, and changed dependencies.
- Manage lightweight instruction packs for team and user conventions in agent instruction files.
- Run auto-sync safely and stop visibly on dirty states or conflicts.
- Fully support macOS and Linux.
- Provide a CLI that is friendly for humans and machine-readable for automation.

## 4. Non-Goals

- No public or private skill registry as a product of its own.
- No automatic activation of project-specific skill sets when entering a code repository.
- No agent-specific skill content variants as a core feature.
- No generic settings sync, dotfile management, editor configuration sync, shell profile management, or linter/formatter configuration distribution.
- No custom secret or credential management.
- No Windows support in the scope of this RFC.
- No automatic transpilation between agent skill formats.
- No implicit migration of existing agent skill directories into managed state.

## 5. Terms

**Skill**  
A skill package with `SKILL.md` as its entry point and optional additional files.

**Agent Asset**  
A managed artifact intended for AI-agent use. In this RFC, supported agent assets are skills and instruction packs.

**Instruction Pack**  
A versioned Markdown artifact containing standing agent-facing conventions, such as team engineering defaults or personal communication preferences. Instruction packs are not executable tool configuration.

**Managed Instruction Block**  
A clearly marked block inserted by skillmgr into an agent instruction file. Skillmgr may update or remove only blocks it owns.

**Source**  
A location skillmgr reads agent assets from. Examples: a private local repository, a team Git repository, or an external Git source.

**Store**  
The local area managed by skillmgr, for example `~/.skillmgr`, where sources are checked out, locks are stored, and local skills are versioned.

**Target**  
An agent location into which skillmgr materializes managed assets. For skills, this is usually a skill directory. For instruction packs, this is usually an agent instruction file such as `AGENTS.md`, `CLAUDE.md`, or an agent-specific global instruction file.

**Materialization**  
The process of making resolved agent assets visible in agent targets through symlinks or managed instruction blocks.

**Managed Skill**  
A skill whose symlink or store entry is managed by skillmgr.

**Unmanaged Skill**  
A skill that exists in an agent target but was not created by skillmgr.

**Local Skill**  
A private user skill in the local skillmgr source.

**Team Skill**  
A skill from a trusted team source.

**External Skill**  
A skill from an external Git source referenced by another source and pinned through a lockfile.

**Catalog Source**  
An external Git repository that may contain multiple skills. Skillmgr can inspect the repository, build an inventory, and select only specific skills from it.

**Selected Skill**  
A skill explicitly chosen from a catalog source for inclusion in the resolved skill set.

**Inventory**  
The discovered set of agent assets in a source or catalog source, including paths, names, stable IDs when available, metadata, content fingerprints, and declared dependencies.

**Shadowed Skill**  
A skill that is not materialized because another source with higher priority provides the same skill name.

**Dirty Skill**  
A skill from a Git source whose working tree contains local changes.

## 6. Core Model

Skillmgr separates three roles:

1. **The store is the managed-state source of truth**  
   Git checkouts, private local skills, locks, source metadata, and materialization state live in the store.

2. **Agent targets are output surfaces**  
   Agent skill directories contain symlinks to resolved skills. Existing unmanaged skills remain untouched.

3. **Team and external sources are versioned inputs**  
   Team repositories and external Git sources provide skills and instruction packs. Source order and locks determine what gets materialized.

Typical data flow:

```text
Git sources + local source
        |
        v
skillmgr resolver
        |
        v
resolved asset set + user lock
        |
        v
symlinks and managed instruction blocks in configured agent targets
```

## 7. Local Store

The default store lives at:

```text
~/.skillmgr/
```

Proposed structure:

```text
~/.skillmgr/
  config.toml
  lock.toml
  state.toml
  local/
    .git/
    skills/
    instructions/
  sources/
    <source-id>/
      checkout/
      lock.toml
  targets/
    registry.toml
  logs/
```

`config.toml` is the user configuration.  
`lock.toml` is the resolved user lock for the sum of all active sources.  
`state.toml` contains materialization state, known symlinks, and non-canonical runtime state.  
`local/` is a private local Git repository for user skills and instruction packs.  
`sources/` contains Git checkouts for team and external sources.

The exact internal layout may change in later versions. The stable public surface is the CLI behavior, configuration format, and lock semantics.

## 8. Configuration

Skillmgr uses TOML for user, source, and lock configuration.

Example `~/.skillmgr/config.toml`:

```toml
[settings]
store_version = 1
autosync = true
sync_interval = "daily"

[[sources]]
id = "local"
kind = "local"
path = "~/.skillmgr/local"
priority = 0
enabled = true

[[sources]]
id = "company"
kind = "team"
url = "git@github.com:example/company-skills.git"
priority = 10
trusted = true
enabled = true

[[sources]]
id = "oss"
kind = "team"
url = "git@github.com:example/oss-skills.git"
priority = 20
trusted = true
enabled = true

[[targets]]
id = "codex"
kind = "codex"
path = "~/.codex/skills"
enabled = true

[[targets]]
id = "claude"
kind = "claude"
path = "~/.claude/skills"
enabled = true
```

Priority: smaller numbers win. The local source has the highest default priority so explicitly adopted private skills are effective. If a local skill displaces a team or external skill with the same name, `status` must show that as a local override.

## 9. Source Model

Supported source classes:

- `local`: the user's private local Git repository
- `team`: a trusted team Git repository
- `external`: a Git source referenced and pinned by another source
- `catalog`: an external Git repository that exposes multiple selectable skills

Team sources may declare external and catalog sources. If a team source is trusted, that trust extends to the external and catalog sources it declares. Sources added directly by the user must be added explicitly.

Catalog sources are treated as offer surfaces, not as all-or-nothing dependencies. A team source may inspect a catalog repository and select only the skills it wants to expose. Skillmgr still checks out or caches the full repository as needed, but only selected skills enter the resolved skill set.

Sources may also provide instruction packs. Instruction packs are selected explicitly or by source policy, then rendered into managed blocks in configured agent instruction files.

Example source file inside a team repository:

```toml
[source]
id = "company"
name = "Company Skills"
kind = "team"

[[external]]
id = "vendor-writing"
url = "https://github.com/vendor/writing-skills.git"
ref = "main"
path = "skills"

[[catalog]]
id = "marketing-skills"
url = "https://github.com/example/marketing-skills.git"
ref = "main"

[[catalog.select]]
catalog = "marketing-skills"
skill = "positioning"
path = "skills/positioning"

[[catalog.select]]
catalog = "marketing-skills"
skill = "launch-copy"
path = "skills/launch-copy"

[[instructions]]
id = "company.engineering-defaults"
path = "instructions/engineering-defaults.md"
topics = ["engineering", "tooling"]
priority = 10
```

External sources are not installed as floating references. They are pinned to concrete commits through source lockfiles.

Catalog selections should prefer stable skill IDs from `SKILL.md` frontmatter when available. If no stable ID exists, selection falls back to repository path and folder name. Path-based selections are more fragile and must surface stronger warnings when upstream structure changes.

## 10. Multi-Skill Catalog Repositories

Some useful upstream repositories are collections of many skills rather than one package. Examples include marketing-oriented skill collections or design skill collections where one repository acts as a browsable catalog. These repositories may not have been authored for skillmgr and may not include skillmgr-specific manifests.

Skillmgr supports these repositories through catalog mode:

1. **Inspect**  
   Skillmgr scans the repository for candidate skill directories. A candidate normally contains `SKILL.md`.

2. **Inventory**  
   Skillmgr records the discovered skills, their paths, stable IDs when present, names, descriptions, content fingerprints, and dependency declarations.

3. **Select**  
   A team or user source explicitly selects the skills it wants to expose. Unselected skills remain visible in catalog status but are not materialized.

4. **Pin**  
   The catalog repository commit and selected skill paths or IDs are pinned in a lockfile.

5. **Update**  
   Updating a catalog compares the old and new inventory before changing the resolved skill set.

Catalog support must distinguish four update outcomes:

- `new_available`: upstream added a skill that is not selected yet
- `selected_changed`: a selected skill changed content or metadata
- `selected_moved`: a selected skill appears to have moved to a new path
- `selected_removed`: a selected skill no longer exists in the updated inventory

`new_available` is informational. `selected_changed` is reviewable through the normal lockfile update flow. `selected_moved` may be auto-reconciled only when the stable skill ID matches. `selected_removed` blocks auto-sync for that selection until the user or team updates the manifest.

Catalog repositories may contain internal skill dependencies. Skillmgr should support explicit dependency declarations in `SKILL.md` frontmatter:

```markdown
---
id: example.launch-copy
name: launch-copy
requires:
  - example.positioning
---
```

If a selected skill declares a required skill from the same catalog, skillmgr should warn when the dependency is not also selected. For trusted team sources, the team manifest may choose one of these policies per catalog:

- `warn`: report missing dependencies but allow selection
- `include`: automatically include declared required skills
- `block`: reject selections with missing required skills

The default policy is `warn`, because many existing repositories do not have reliable machine-readable dependency metadata. Skillmgr may add best-effort warnings when skill text mentions another skill by name, but text inference must never be the only blocker.

## 11. Lockfiles

There are two lockfile levels:

1. **Source lockfile**  
   A source pins its external sources to concrete commits or versions.

2. **User lockfile**  
   The user lock describes the resolved sum of all active sources, including winners, shadowed skills, active instruction packs, and commit hashes.

Normal `sync` respects existing pins.  
`update` creates targeted lockfile changes and can turn them into PRs.

For catalog sources, lockfiles must also record the selected skills and the inventory snapshot used to resolve them. This allows skillmgr to detect upstream structure drift on update rather than silently losing or adding skills.

Example resolved user lock:

```toml
version = 1

[[resolved]]
id = "company:copy-editing"
name = "copy-editing"
source = "company"
path = "~/.skillmgr/sources/company/checkout/skills/copy-editing"
commit = "abc123"
status = "active"

[[resolved]]
id = "oss:copy-editing"
name = "copy-editing"
source = "oss"
path = "~/.skillmgr/sources/oss/checkout/skills/copy-editing"
commit = "def456"
status = "shadowed"
shadowed_by = "company:copy-editing"

[[catalogs]]
id = "marketing-skills"
source = "company"
url = "https://github.com/example/marketing-skills.git"
commit = "789abc"
inventory_hash = "sha256:..."

[[catalogs.selected]]
catalog = "marketing-skills"
id = "example.positioning"
name = "positioning"
path = "skills/positioning"
content_hash = "sha256:..."
status = "active"

[[instructions]]
id = "company.engineering-defaults"
name = "Engineering Defaults"
source = "company"
path = "instructions/engineering-defaults.md"
content_hash = "sha256:..."
status = "active"
```

## 12. Skill Identity and Metadata

New skills should have a stable ID in the `SKILL.md` frontmatter. Existing skills without an ID are recognized by folder name.

Example:

```markdown
---
id: company.copy-editing
name: copy-editing
description: Edit and improve marketing copy.
owners:
  - content-platform
---

# Copy Editing
...
```

Minimum common denominator:

- `SKILL.md` exists in the skill folder.
- `name` or the folder name determines the display name.
- `id` is recommended but not immediately required for migration.

Optional dependency metadata:

- `requires` lists skill IDs or source-qualified names that should be available together with this skill.
- Missing declared dependencies produce at least a warning and may block when the source policy is `block`.

Agent-specific skill content variants are not a core feature. The skill itself remains one shared package. Adapters may only know installation details such as target path or link strategy.

## 13. Instruction Packs

Instruction packs are versioned Markdown artifacts for standing agent-facing conventions. They are useful for guidance that should be available across repositories or teams but is not itself a skill.

Examples:

- engineering defaults: "Use OXLint for linting and OXFMT for formatting."
- language defaults: "Prefer TypeScript for application code."
- architecture defaults: "Use Rust for performance-sensitive non-browser code."
- review defaults: "Call out behavioral regressions before style nits."
- personal defaults: "Use concise US English for issue comments."

Instruction packs are explicitly not settings sync. They must not distribute `.oxlintrc`, editor settings, shell profiles, package-manager configuration, formatter configuration, credentials, or arbitrary dotfiles.

Suggested source layout:

```text
company-skills/
  skills/
  instructions/
    engineering-defaults.md
    frontend-defaults.md
    review-style.md
```

Suggested instruction pack frontmatter:

```markdown
---
id: company.engineering-defaults
name: Engineering Defaults
topics:
  - engineering
  - tooling
priority: 10
---

Use OXLint for linting and OXFMT for formatting. Prefer TypeScript for application code. Use Rust for performance-sensitive non-browser code that does not need to run in the browser.
```

Instruction packs are materialized into agent instruction files as managed blocks:

```markdown
<!-- skillmgr:start company.engineering-defaults -->
Use OXLint for linting and OXFMT for formatting.
<!-- skillmgr:end company.engineering-defaults -->
```

Rules:

- skillmgr may only create, update, reorder, or remove blocks marked with its own `skillmgr:start` and `skillmgr:end` markers.
- skillmgr must never rewrite unmarked content in files such as `AGENTS.md`, `CLAUDE.md`, or agent-specific global instruction files.
- project-local instruction files remain owned by the project repository and are not replaced by global instruction packs.
- team instruction packs and local instruction packs may both be active.
- source priority decides ordering when multiple instruction packs target the same file.
- topic overlap is reported by `status` and `doctor`, but skillmgr does not try to infer semantic contradictions.
- local instruction packs may override or supplement team instruction packs, but overrides must be visible.

The resolver should treat instruction packs as a separate asset type from skills. They share source, lock, trust, and update mechanics with skills, but their materialization strategy is managed text blocks rather than symlinks.

## 14. Target Registry

Skillmgr knows default targets for prominent agents:

- Codex
- Claude
- Cursor
- OpenCode
- OpenClaw
- Hermes
- generic folder-based agents

The registry provides default paths, detection rules, link policy, and instruction-file policy. Users can manually add or override targets.

`target detect` searches existing skill directories and reports:

- recognized known targets
- possible generic targets
- missing but known default paths
- unmanaged skills in existing targets
- existing agent instruction files that can receive managed instruction blocks

Detection must not change files.

## 15. Materialization

Managed skills are installed into every enabled target through symlinks. Instruction packs are rendered into configured agent instruction files as managed blocks.

Rules:

- skillmgr only creates symlinks to resolved store paths.
- skillmgr only writes instruction text inside its own managed block markers.
- skillmgr does not replace real directories or files without an explicit action.
- skillmgr only removes symlinks that it created and knows in `state.toml`.
- skillmgr only removes instruction blocks that it created and knows in `state.toml`.
- Unmanaged files and directories remain untouched.
- Unmarked instruction-file content remains untouched.
- Broken symlinks are reported by `doctor` and `status`.
- A target may contain only one active managed link per skill name.
- A target instruction file may contain multiple managed instruction blocks, ordered by source priority and pack priority.

If a skill is removed from a source, skillmgr removes its own symlink from targets during the next sync. The actual source content remains available only through Git history or the store checkout.

If an instruction pack is removed from a source, skillmgr removes only its own managed block from configured instruction files. Manual content outside skillmgr markers must never be removed.

## 16. Status Model

`status` should be able to show skills in at least these states:

- `managed`: installed by skillmgr
- `unmanaged`: exists in a target but is not managed by skillmgr
- `local`: from the user's local source
- `team`: from a team source
- `external`: from a pinned external source
- `shadowed`: inactive because another source has higher priority
- `dirty`: source working tree has local changes
- `conflicted`: conflict cannot be resolved safely
- `orphaned`: link or state points to a removed source
- `protected`: `.local` or another protection rule prevents automatic action
- `new_available`: a catalog source contains a new unselected skill
- `selected_removed`: a selected catalog skill disappeared upstream
- `selected_moved`: a selected catalog skill appears to have moved
- `dependency_missing`: a selected skill declares a dependency that is not selected or otherwise available
- `instruction_active`: an instruction pack is active and materialized
- `instruction_topic_overlap`: multiple active instruction packs declare the same topic
- `instruction_block_drift`: a managed instruction block differs from the resolved source content
- `instruction_block_orphaned`: a managed instruction block references a removed instruction pack

Conflicts may be visible without always being blocking. Equal skill names are resolved deterministically by source priority but reported as shadowing.

## 17. CLI Surface

All relevant commands should be interactive for humans and stable for automation.

Global flags:

```text
--json       machine-readable output
--yes        non-interactive confirmation for safe actions
--dry-run    show planned actions without changing anything
--store      alternate store path
```

### 17.1 `skillmgr init`

Sets up the store, local source, configuration, and optional targets.

Behavior:

- creates `~/.skillmgr`
- initializes `~/.skillmgr/local` as a private Git repository
- detects existing agent targets
- detects existing agent instruction files
- shows unmanaged skills
- does not change existing agent files
- does not rewrite existing instruction files
- offers target linking

### 17.2 `skillmgr target detect`

Detects known agent skill directories, instruction files, and unmanaged skills.

No mutation.

### 17.3 `skillmgr target link`

Enables a target for materialization.

Examples:

```text
skillmgr target link codex
skillmgr target link generic --id my-agent --path ~/.my-agent/skills
```

### 17.4 `skillmgr target unlink`

Disables a target. Optionally removes skillmgr-owned symlinks and managed instruction blocks from that target. Unmanaged files and unmarked instruction content are never removed.

### 17.5 `skillmgr source add`

Adds a Git or local source.

Examples:

```text
skillmgr source add company git@github.com:example/company-skills.git
skillmgr source add oss https://github.com/example/oss-skills.git --priority 20
```

New direct sources must be added explicitly by the user.

Catalog examples:

```text
skillmgr source add-catalog marketing-skills https://github.com/example/marketing-skills.git
skillmgr source inspect marketing-skills
skillmgr source select marketing-skills positioning
skillmgr source select marketing-skills launch-copy --path skills/launch-copy
```

### 17.6 `skillmgr source list`

Lists sources, priority, trust, sync status, head commit, and dirty state.

### 17.7 `skillmgr source remove`

Disables or removes a source. On the next sync, only skillmgr-owned symlinks are removed.

### 17.8 `skillmgr source priority`

Sets source order. This order decides equal-name skill conflicts.

### 17.9 `skillmgr status`

Shows:

- active skills
- local skills
- unmanaged skills
- shadowed skills
- dirty sources
- conflicted skills
- orphaned symlinks
- protected `.local` skills
- catalog skills that are new, moved, removed, changed, or missing declared dependencies
- active instruction packs
- instruction topic overlap
- managed instruction block drift
- orphaned managed instruction blocks
- target problems

`status --json` is the foundation for agents, CI, or later UI layers.

### 17.10 `skillmgr sync`

Updates sources safely and materializes the resolved asset set.

Normal behavior:

- fetches/pulls only clean sources
- respects lockfiles
- respects catalog selections and does not materialize unselected catalog skills
- creates or updates symlinks
- creates or updates managed instruction blocks
- removes only known owned symlinks
- removes only known owned instruction blocks
- reports shadowing
- reports instruction topic overlap
- reports new catalog skills
- stops on dirty state, broken locks, selected catalog skill removal, instruction block drift during auto-sync, or unresolved conflicts

`sync --auto` is used by the scheduler and must be more conservative than interactive sync. It must not make risky decisions.

### 17.11 `skillmgr instruction list`

Lists available and active instruction packs from all active sources.

### 17.12 `skillmgr instruction enable`

Enables an instruction pack for materialization.

Example:

```text
skillmgr instruction enable company.engineering-defaults
```

### 17.13 `skillmgr instruction disable`

Disables an instruction pack and removes only its owned managed blocks during the next sync.

### 17.14 `skillmgr adopt`

Adopts unmanaged skills into the local source.

Default flow:

1. detect unmanaged skill
2. copy skill into `~/.skillmgr/local/skills/<name>`
3. leave the local source dirty or optionally offer a commit
4. offer to replace the original folder with a symlink
5. do not remove existing files without confirmation

`.local` skills remain protected. Adoption is possible but always explicit.

### 17.15 `skillmgr promote`

Turns a local skill or dirty team edit into a team contribution.

Vision:

- PR-first workflow
- with multiple team sources, `--target` or interactive selection is required
- creates branch, commit, and GitHub PR when auth is available
- uses existing Git and `gh` credentials
- GitLab and other forges remain future adapters

Examples:

```text
skillmgr promote copy-editing --target company
skillmgr promote copy-editing --from-dirty --target company
```

### 17.16 `skillmgr resolve`

Assisted conflict resolution.

Supported cases:

- dirty team skill: commit+PR, stash, local override, or discard
- shadowed skill: change priority, assign alias, disable source, or ignore
- broken symlink: rematerialize or remove link
- orphaned source: restore source or remove link
- lock conflict: regenerate lock or abort update
- instruction block drift: restore from source, keep local edit as local instruction pack, or remove managed block

### 17.17 `skillmgr update`

Updates external pins and creates reviewable lockfile changes.

Default:

- no floating update through normal `sync`
- targeted updates by source or skill
- catalog updates compare old and new inventories before changing selected skills
- new unselected catalog skills are reported but not automatically enabled
- removed selected catalog skills block until selection is changed or intentionally removed
- optional GitHub PR for lockfile changes

### 17.18 `skillmgr doctor`

Checks:

- store structure
- config syntax
- lockfile consistency
- source reachability
- Git auth
- GitHub auth for PR flows
- target paths
- broken symlinks
- malformed managed instruction block markers
- managed instruction blocks whose source no longer exists
- instruction topic overlap
- unmanaged/protected skills
- catalog selections whose paths no longer exist
- selected skills with missing declared dependencies
- catalog inventories that cannot be scanned
- scheduler installation
- unknown targets

## 18. Autosync

`skillmgr autosync install` sets up an OS-native scheduler:

- macOS: launchd
- Linux: systemd timer, falling back to cron only when necessary

Default:

- daily
- with jitter
- runs `skillmgr sync --auto`
- writes logs into the store
- reports blocking conflicts visibly through `status` and `doctor`

Autosync must not make interactive decisions or overwrite dirty states.

## 19. Conflict Model

### 19.1 Equal Skill Names

If multiple sources provide the same skill name, the source with the highest user priority wins. The others become `shadowed`. If the local source wins, the winner is additionally shown as a `local override`.

This is not a hard error, but it must be visible:

```text
active: company/copy-editing
shadowed: oss/copy-editing by company/copy-editing
```

```text
active: local/copy-editing (local override)
shadowed: company/copy-editing by local/copy-editing
```

### 19.2 Dirty Team Skills

If a symlinked team skill is edited locally, the underlying source becomes dirty. `sync --auto` blocks for that source or skill.

The user can then choose through `resolve`:

- `commit+PR`
- `stash`
- `local override`
- `discard`

None of these actions happen automatically.

### 19.3 `.local`

A skill with a `.local` suffix or matching protection marker is private and protected.

Rules:

- never overwrite automatically
- never remove automatically
- report visibly on conflicts
- adoption or promotion only explicitly

`.local` is a legacy and protection marker, not the primary management mechanism. The primary mechanism for private skills is the local source.

### 19.4 Orphans

An orphan exists when a target symlink or state entry points to a source or skill that no longer exists.

Skillmgr may remove its own orphaned symlinks, but it should make that visible in `status` and `doctor`.

### 19.5 Catalog Drift

Catalog drift happens when an upstream multi-skill repository changes shape between pinned commits.

Rules:

- newly discovered unselected skills are informational
- changed selected skills are handled through normal update review
- moved selected skills may be reconciled automatically only when stable IDs match
- removed selected skills block auto-sync for that selection
- missing declared dependencies warn by default and may block under stricter source policy

### 19.6 Instruction Block Drift

Instruction block drift happens when a managed instruction block in an agent instruction file differs from the resolved instruction pack content.

Rules:

- unmarked content is never considered drift and must not be changed
- interactive sync may offer to restore the managed block from source
- auto-sync reports and blocks on drift instead of overwriting local edits
- `resolve` may convert local block edits into a local instruction pack
- orphaned managed blocks may be removed only when their markers are intact and the block is known in `state.toml`

## 20. Security and Trust Model

Skillmgr treats skills as executable agent instructions with supply-chain relevance.

Principles:

- no custom secret management
- no generic settings sync or dotfile management
- no silent floating updates of external sources
- no silent enabling of new catalog skills
- no silent rewriting of unmarked agent instruction content
- new direct user sources only through explicit action
- trusted team sources may extend trust to declared external and catalog sources
- lockfile changes are reviewable
- PR flows use existing GitHub auth
- project repositories must not automatically activate global skills

`doctor` should surface risks, for example:

- source without lock
- external source with floating ref and no pin
- selected catalog skill that disappeared upstream
- selected catalog skill with missing declared dependency
- managed instruction block drift
- instruction topic overlap
- malformed managed instruction block markers
- broken symlink
- unmanaged skill with name conflict
- dirty team checkout

## 21. Project-Specific Skills

Real project-specific skills remain in the code repository they belong to. Many agents already discover those project skills by themselves. Skillmgr should not automatically switch global user targets based on the current project.

In the future, skillmgr may optionally detect project skills and show them informatively in `doctor` or `status`. That is not a core feature of this RFC.

## 22. Promotion Flow

Promotion is the feedback path from local skill growth into team standards.

### 22.1 Local Skill to Team Skill

```text
skillmgr adopt writing-helper
skillmgr promote writing-helper --target company
```

Expected behavior:

- target team source is explicit
- skill is validated
- branch is created
- files are copied into the team repository
- commit is created
- GitHub PR is created when auth is available

### 22.2 Dirty Team Edit to PR

```text
skillmgr status
skillmgr resolve copy-editing --commit-pr --target company
```

Expected behavior:

- dirty state is detected
- sync blocks until the user decides
- user can submit the change as a PR
- after a successful PR flow, the source returns to a defined state

## 23. Validation

Validation is warn-by-default.

Warnings:

- missing recommended metadata
- unknown frontmatter fields
- missing owner entry
- incomplete description
- skill without stable ID
- path-based catalog selection without stable skill ID
- missing declared dependency under `warn` policy
- instruction topic overlap
- managed instruction block drift during interactive sync

Blockers:

- missing `SKILL.md`
- broken source path
- symlink target outside allowed store paths
- unparseable config or lockfiles
- dirty state during auto-sync
- unresolved conflict in target path
- selected catalog skill removed upstream
- missing declared dependency under `block` policy
- malformed managed instruction block markers
- managed instruction block drift during auto-sync

## 24. Acceptance Criteria

- `init` detects existing agent skill directories, shows unmanaged skills, and does not change them.
- `init` detects existing agent instruction files and does not rewrite them.
- Multiple sources resolve deterministically through user priority.
- The user lock makes the resolved asset set reproducible.
- Instruction packs are resolved as a separate asset type from skills.
- Instruction packs render only into skillmgr-owned managed blocks.
- Unmarked content in `AGENTS.md`, `CLAUDE.md`, or agent-specific instruction files is never rewritten.
- Topic overlap between instruction packs is reported without semantic inference.
- Auto-sync blocks on managed instruction block drift.
- Catalog sources allow selecting only specific skills from a multi-skill repository.
- Catalog updates report new upstream skills without enabling them automatically.
- Catalog updates block when a selected skill disappears upstream.
- Catalog updates reconcile moved selected skills only when stable IDs match.
- Missing declared dependencies are reported, and policy controls whether they warn or block.
- Equal skill names are reported visibly as active/shadowed.
- `sync --auto` updates clean sources and blocks dirty states without data loss.
- Managed skills are materialized through symlinks.
- Removed skills delete only skillmgr-owned symlinks, never real files.
- `adopt` copies into the local source first and replaces the original folder only after confirmation.
- `.local` skills remain protected.
- `promote` creates a PR for local skills or dirty team edits against the explicitly selected team target.
- `doctor` finds broken symlinks, missing auth, broken locks, and unknown targets.
- macOS and Linux are fully supported.

## 25. Open Questions

These points are intentionally not final and should be refined in follow-up RFCs or before implementation slices:

- Exact `SKILL.md` frontmatter fields and naming conventions
- Exact lockfile schema and compatibility rules for schema upgrades
- Exact catalog inventory schema and move-detection heuristics
- Exact instruction pack frontmatter fields and target-file mapping rules
- Whether instruction packs should be enabled explicitly by default or selected by source policy
- How much block drift recovery should exist in v1
- Whether `requires` should use only stable skill IDs or also source-qualified names
- Whether dependency policy should be global, per source, per catalog, or per selected skill
- Which agent targets are implemented in v1
- Whether local source changes should be committed automatically or always remain manual
- How aliasing should work for shadowed skills when a user wants both variants in parallel
- How detailed `resolve` must be in v1
- Whether `promote` uses `gh` directly or an internal GitHub API abstraction
- How notifications for blocked auto-syncs should work

## 26. Suggested V1 Slice

Although this RFC describes the full vision, the first implementation should be narrow. V1 proves the architecture — store, resolver, materialization, reconciliation — on the core single- and multi-source skill loop, before taking on the features that carry the most unresolved design questions.

### 26.1 V1 scope

- `init`
- `target detect/link/unlink`
- `source add/list/priority` for `local` and `team` sources
- `status`
- `sync`
- `adopt`
- `doctor`
- TOML config
- local store
- symlink materialization
- deterministic multi-source resolution
- user lockfile for the resolved asset set
- warnings for shadowing and dirty state

### 26.2 Deferred to V1.1

Catalog support and instruction packs are deferred to V1.1, not because they are unimportant, but because they depend on design decisions that are still open: the skill identity key (§12, §19.1), managed/unmanaged name-collision resolution (§15, §19), and the instruction-pack target-file mapping (§13, §25). Shipping them before those decisions are made would bake in guesses. Pulling them out of V1 keeps the first slice focused on the riskiest engine code and resolves the tension between §25 (mapping rules still open) and the original V1 scope.

- basic catalog source inspection and explicit selection
- lockfile entries for selected catalog skills
- warnings for new catalog skills and missing declared dependencies
- basic instruction pack discovery and managed block rendering
- warnings for instruction topic overlap

### 26.3 Suggested implementation sequence

The V1 scope is a feature set, not an order. A workable sequence:

1. store layout, TOML config parsing, and `init`
2. target registry and `target detect/link/unlink`
3. inventory scan and single-source `sync` with symlink materialization
4. deterministic multi-source resolution (priority, shadowing) and `status`
5. `adopt` into the local source, with optional symlink replacement
6. `doctor` diagnostics

This sequence front-loads the store and materializer (the highest-risk filesystem code) and adds multi-source behavior only once a single source materializes safely.

### 26.4 Not in early slices

- full automatic scheduler
- `update` with lockfile PRs
- full `resolve` assistant
- automatic catalog move reconciliation
- strict dependency enforcement beyond warnings
- advanced instruction block drift recovery
- full `promote` PR flow
- all agent targets
- forge adapters beyond GitHub

The early slices should prove the architecture without implementing the entire vision at once.

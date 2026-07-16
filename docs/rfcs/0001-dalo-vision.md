# RFC 0001: Dalo Vision

Status: Draft  
Date: 2026-06-23  
Author: Sebastian + Codex  

## 1. Summary

Dalo is a CLI-first manager for AI-agent skills on macOS and Linux. It centralizes skills and instruction packs from multiple Git-based sources, resolves them into one final asset set, materializes skills into configured agent skill directories through symlinks, and renders instruction packs into managed agent-instruction blocks.

The vision addresses six core problems:

1. Teams need shared, versioned skills that can update automatically.
2. Users often create and refine skills directly with agents inside existing agent skill directories.
3. Individual users need private skills that do not automatically belong to the team.
4. A user may belong to multiple teams or skill sources, and the final available asset set should be the sum of all active sources.
5. Useful third-party repositories often contain many skills, of which a team may only want to use a selected subset.
6. Teams and individuals also need lightweight shared conventions that belong in agent instruction files rather than in executable tool configuration.

Dalo is intentionally not a public registry service and not a project-profile switcher. The central product idea is a robust source resolver with a local store, safe sync, visible conflicts, adoption of existing skills, and PR-first promotion into team repositories.

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

Instead, dalo manages its own store and uses agent directories as materialization targets and discovery locations.

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
- Run scheduled sync safely and stop visibly on dirty states or conflicts.
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
A clearly marked block inserted by dalo into an agent instruction file. Dalo may update or remove only blocks it owns.

**Source**  
A location dalo reads agent assets from. Examples: a private local repository, a team Git repository, or an external Git source.

**Store**  
The local area managed by dalo, for example `~/.dalo`, where sources are checked out, locks are stored, and local skills are versioned.

**Target**  
An agent location into which dalo materializes managed assets. For skills, this is usually a skill directory. For instruction packs, this is usually an agent instruction file such as `AGENTS.md`, `CLAUDE.md`, or an agent-specific global instruction file.

**Materialization**  
The process of making resolved agent assets visible in agent targets through symlinks or managed instruction blocks.

**Managed Skill**  
A skill whose symlink or store entry is managed by dalo.

**Unmanaged Skill**  
A skill that exists in an agent target but was not created by dalo.

**Local Skill**  
A private user skill in the local dalo source.

**Team Skill**  
A skill from a trusted team source.

**External Skill**  
A skill from an external Git source referenced by another source and pinned through a lockfile.

**Catalog Source**  
An external Git repository that may contain multiple skills. Dalo can inspect the repository, build an inventory, and select only specific skills from it.

**Selected Skill**  
A skill explicitly chosen from a catalog source for inclusion in the resolved skill set.

**Inventory**  
The discovered set of agent assets in a source or catalog source, including paths, names, stable IDs when available, metadata, content fingerprints, and declared dependencies.

**Shadowed Skill**  
A skill that is not materialized because another source with higher priority provides the same slot name.

**Unlinked Skill**
A user-facing status for a managed skill that exists in the store but is not linked into an agent target. Shadowing is one possible reason a skill is unlinked.

**Blocked Same-Name Skill**
A managed skill that dalo would otherwise materialize, but cannot link because the target slot is already occupied by a non-dalo entry such as an unmanaged skill, project skill, or foreign symlink.

**Dirty Skill**  
A skill from a Git source whose working tree contains local changes.

## 6. Core Model

Dalo separates three roles:

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
dalo resolver
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
~/.dalo/
```

Proposed structure:

```text
~/.dalo/
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
      lock.toml        # (V1.1: per-source pinned lock)
```

V1 keeps target configuration in `state.toml`; the dedicated `targets/registry.toml`
and per-source `lock.toml` files above are planned for V1.1 and are not written yet.

`config.toml` is the user configuration.  
`lock.toml` is the resolved user lock for the sum of all active sources.  
`state.toml` contains materialization state, known symlinks, and non-canonical runtime state.  
`local/` is a private local Git repository for user skills and instruction packs.  
`sources/` contains Git checkouts for team and external sources.

The exact internal layout may change in later versions. The stable public surface is the CLI behavior, configuration format, and lock semantics.

## 8. Configuration

Dalo uses TOML for user, source, and lock configuration.

Example `~/.dalo/config.toml`:

```toml
[settings]
store_version = 1
autosync = true
sync_interval = "daily"

[[sources]]
id = "local"
kind = "local"
path = "~/.dalo/local"
priority = 0
enabled = true

[[sources]]
id = "company"
kind = "team"
url = "git@github.com:example/company-skills.git"
branch = "main"
update_policy = "track"
priority = 10
trusted = true
enabled = true

[[sources]]
id = "oss"
kind = "team"
url = "git@github.com:example/oss-skills.git"
branch = "main"
update_policy = "track"
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

Priority: smaller numbers win. The local source has the highest default priority so explicitly adopted private skills are effective. If a local skill displaces a team or external skill with the same slot name, `status` must show that as a local override.

Team sources normally track a configured branch. `sync` refreshes clean tracking team sources before resolving and materializing skills. Sources that should not move during normal `sync` can use a pinning policy in later slices; external and catalog sources are pinned through source lockfiles by default.

## 9. Source Model

Supported source classes:

- `local`: the user's private local Git repository
- `team`: a team Git repository whose updates may be followed by `sync`
- `external`: a Git source referenced and pinned by another source
- `catalog`: an external Git repository that exposes multiple selectable skills

Team sources may declare external and catalog sources. That declaration makes the sources visible to dalo, but it does not by itself approve newly active skills. Any skill that would become active for the first time needs local approval unless it is covered by an existing author-, org-, source-, or skill-level approval rule. Sources added directly by the user must be added explicitly.

Catalog sources are treated as offer surfaces, not as all-or-nothing dependencies. A team source may inspect a catalog repository and select only the skills it wants to expose. Dalo still checks out or caches the full repository as needed, but only selected skills enter the resolved skill set.

Sources may also provide instruction packs. Instruction packs are explicitly enabled by the user or team manifest, then rendered into managed blocks in configured agent instruction files.

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
version = "789abcdef0123456789abcdef0123456789abcd"
skills = ["+positioning", "+launch-copy"]

[[instructions]]
id = "company.engineering-defaults"
path = "instructions/engineering-defaults.md"
topics = ["engineering", "tooling"]
priority = 10
```

External sources are not installed as floating references. They are pinned to concrete commits through source lockfiles.

Catalog selections should prefer stable skill IDs from `SKILL.md` frontmatter when available. If no stable ID exists, selection falls back to repository path and folder name. Path-based selections are more fragile and must surface stronger warnings when upstream structure changes.

## 10. Multi-Skill Catalog Repositories

Some useful upstream repositories are collections of many skills rather than one package. Examples include marketing-oriented skill collections or design skill collections where one repository acts as a browsable catalog. These repositories may not have been authored for dalo and may not include dalo-specific manifests.

Dalo supports these repositories through catalog mode:

1. **Inspect**  
   Dalo scans the repository for candidate skill directories. A candidate normally contains `SKILL.md`.

2. **Inventory**  
   Dalo records the discovered skills, their paths, stable IDs when present, names, descriptions, content fingerprints, and dependency declarations.

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

`new_available` is informational. `selected_changed` is reviewable through the normal source-refresh flow. `selected_moved` may be auto-reconciled only when the stable skill ID matches. `selected_removed` blocks scheduled sync for that selection until the user or team updates the manifest.

Catalog repositories may contain internal skill dependencies. Dalo should support explicit dependency declarations in `SKILL.md` frontmatter:

```markdown
---
id: example.launch-copy
name: launch-copy
description: Helps create launch copy from positioning inputs.
owners:
  - marketing
tags:
  - marketing
  - launch
requires:
  - example.positioning
---
```

V1 frontmatter interpretation is intentionally small:

- `id`: stable identity, recommended for new managed skills
- `name`: slot name and visible install name
- `description`: human and agent-facing summary
- `requires`: required skill references
- `owners`: maintainers or responsible team/user handles
- `tags`: optional discovery labels

Unknown fields are tolerated and preserved by source repositories, but V1 does not interpret them. For existing skills without `id`, dalo falls back to slot name and path, with stronger drift warnings.

If a selected skill declares required skills from the same source or catalog, those required skills become part of the effective selection automatically. This expansion is transitive: required skills may require other skills, and the resolver walks the full closure before materialization.

Required skills are still subject to approval. If a required skill would become active for the first time and is not covered by an existing skill-, source-, author-, or org-level approval, the dependent skill is not materialized until approval is granted.

If a required same-source skill is missing, blocked by a same-name target entry, unlinked because of conflict, pending approval, or otherwise not linkable, the dependent skill is blocked during preflight. Dalo must not materialize a selected skill whose declared required closure cannot be linked consistently.

Cross-source dependencies are checked only and are never auto-installed across source boundaries. Dalo may add best-effort warnings when skill text mentions another skill by name, but text inference must never be the only blocker.

## 11. Lockfiles

There are two lockfile levels:

1. **Source lockfile**  
   A source pins its external sources to concrete commits or versions.

2. **User lockfile**  
   The user lock describes the resolved sum of all active sources, including winners, shadowed skills, active instruction packs, and commit hashes.

Normal `sync` refreshes clean tracking team sources, respects pinned sources, resolves the final asset set, and materializes it into targets. For tracking team sources, the user lock is a descriptive snapshot of what was active after the last sync. For pinned sources, the lock is prescriptive: `sync` must not move them forward.

`source refresh` creates targeted lockfile changes for pinned external, catalog, or pinned team sources and can turn them into PRs.

For catalog sources, lockfiles must also record the selected skills and the inventory snapshot used to resolve them. This allows dalo to detect upstream structure drift during `source refresh` rather than silently losing or adding skills.

Lockfiles use TOML and include `schema_version`. A newer unsupported major schema version blocks mutation until dalo is upgraded. Unknown minor-version fields are ignored for forward compatibility. `source-lock.toml` is prescriptive for pinned external, catalog, and pinned team sources. The user's `lock.toml` is a resolved snapshot: prescriptive for pinned sources and descriptive for tracking team sources.

Catalog inventory entries record at least stable ID when present, slot name, path, content hash, metadata hash, and declared `requires`. Move detection is automatic only when the stable ID is unchanged. Without a stable ID, path/name/content heuristics may warn about a likely move, but must not silently rewrite the selection.

Example resolved user lock:

```toml
schema_version = 1

[[resolved]]
ref = "company:copy-editing"
slot_name = "copy-editing"
stable_id = "company.copy-editing"
source = "company"
path = "~/.dalo/sources/company/checkout/skills/copy-editing"
commit = "abc123"
status = "active"

[[resolved]]
ref = "oss:copy-editing"
slot_name = "copy-editing"
stable_id = "oss.copy-editing"
source = "oss"
path = "~/.dalo/sources/oss/checkout/skills/copy-editing"
commit = "def456"
status = "unlinked"
reason = "shadowed"
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
slot_name = "positioning"
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

Skill identity has three layers:

1. **Stable ID**
   A long-lived identifier from `SKILL.md` frontmatter, used for dependencies, catalog move detection, drift tracking, and promotion history.

2. **Source-qualified reference**
   Dalo's internal unambiguous reference, with the form `<source-id>:<slot-name>`, for example `company:copy-editing` or `coreyhaines-marketing:positioning`.

3. **Slot name**
   The visible install name and physical target-directory name. This is the conflict and shadowing key because each target directory can contain only one entry with a given name.

New team and catalog skills should have a stable ID in `SKILL.md` frontmatter. Existing skills without an ID are recognized by slot name.

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
- `name` or the folder name determines the slot name.
- slot names must normalize to a safe single path segment.
- `id` is recommended but not immediately required for migration.

The default visible install name is the short slot name. Dalo does not automatically namespace every installed skill with its source. When multiple sources provide the same slot name, source priority decides which one gets the short name and the others become unlinked with reason `shadowed`.

V1 does not provide a parallel alias layer for same-name variants. If users later need two same-name variants at the same time, a future explicit rename/adapt flow can copy or fork one variant under a new slot name and update machine-readable same-source references where possible. Lockfiles, dependency checks, and catalog move detection use stable IDs or source-qualified references, never user-facing aliases.

Optional dependency metadata:

- `requires` lists dependencies that should be available together with this skill. Supported reference forms are stable skill IDs, source-qualified refs, and same-source relative slot names.
- Same-source relative slot names are first-class, not just legacy compatibility. External multi-skill repositories often describe relationships in the local vocabulary of that repository, and dalo should not require patching those upstream skills before they can be used.
- Relative `requires` entries resolve only inside the declaring source or catalog. They do not search all configured sources.
- Cross-source dependencies require a stable ID or source-qualified ref and are checked only; they are never auto-installed across source boundaries.
- Visible aliases are never dependency targets.
- Missing or unlinked declared dependencies block dependent skills during preflight when they are part of the required closure.

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

V1.1 instruction pack frontmatter uses the same small-field approach:

- `id`: stable pack identity
- `name`: display name
- `topics`: coarse overlap labels
- `priority`: ordering within the same target file

Instruction packs are not auto-enabled just because a source provides them. The user or team manifest must explicitly enable a pack and map it to one or more target instruction files. This avoids silently writing team guidance into a user's personal agent files.

Instruction packs are materialized into agent instruction files as managed blocks:

```markdown
<!-- dalo:start company.engineering-defaults -->
Use OXLint for linting and OXFMT for formatting.
<!-- dalo:end company.engineering-defaults -->
```

Managed blocks are the portable baseline for V1.1. Some agents support native file imports or prompt-file references, but support is inconsistent and often agent-specific. Dalo must not assume a shared include syntax across targets. A later adapter may use native includes only when that target's support is verified; the fallback and default behavior remains rendering dalo-owned managed blocks into the configured instruction file.

Rules:

- dalo may only create, update, reorder, or remove blocks marked with its own `dalo:start` and `dalo:end` markers.
- dalo must not emit unverified include/import references as the primary materialization strategy.
- dalo must never rewrite unmarked content in files such as `AGENTS.md`, `CLAUDE.md`, or agent-specific global instruction files.
- project-local instruction files remain owned by the project repository and are not replaced by global instruction packs.
- team instruction packs and local instruction packs may both be active.
- source priority decides ordering when multiple instruction packs target the same file.
- topic overlap is reported by `status` and `doctor`, but dalo does not try to infer semantic contradictions.
- local instruction packs may override or supplement team instruction packs, but overrides must be visible.

Drift handling in the first instruction-pack release is intentionally small. Malformed managed block markers block writes to that file. A drifted managed block blocks scheduled or non-interactive sync. Interactive sync may restore the block from source or leave it as-is. Converting a drifted block into a local instruction pack is a later enhancement.

The resolver should treat instruction packs as a separate asset type from skills. They share source, lock, trust, and update mechanics with skills, but their materialization strategy is managed text blocks rather than symlinks.

## 14. Target Registry

Dalo knows default targets for prominent agents. V1 supports a small verified target set:

| Target | V1 status | Default skill path | Notes |
| --- | --- | --- | --- |
| Codex | supported | `~/.agents/skills` | Codex also discovers repo-scoped `.agents/skills`; dalo manages only the global/user target. |
| Claude Code | supported | `~/.claude/skills` | Claude has explicit precedence rules; same-name project skills may still affect runtime behavior. |
| OpenClaw | supported | `~/.agents/skills` | OpenClaw treats this as personal agent skills; workspace and project-agent skills have higher precedence. |
| Hermes | supported | `~/.hermes/skills` | Hermes uses this as its primary skill source of truth; `~/.agents/skills` can be configured separately as an external directory. |
| generic folder | supported | user-specified | For agents that consume Agent Skills from a directory but do not yet have a verified adapter. |
| Cursor | experimental | unverified | Registry entry may exist, but V1 should not promise full support until discovery behavior is verified. |
| OpenCode | experimental | unverified | Registry entry may exist, but V1 should not promise full support until discovery behavior is verified. |

Multiple target adapters may resolve to the same physical directory. For example, Codex and OpenClaw can both use `~/.agents/skills`. Dalo must canonicalize target paths, de-duplicate identical materialization directories, and still report the logical agents that depend on that directory.

The registry provides default paths, detection rules, link policy, and instruction-file policy. Users can manually add or override targets.

`target detect` searches existing skill directories and reports:

- recognized known targets
- possible generic targets
- missing but known default paths
- unmanaged skills in existing targets
- existing agent instruction files that can receive managed instruction blocks

Detection must not change files.

## 15. Materialization

Managed skills are installed into every enabled target as one directory-level symlink per skill slot. Instruction packs are rendered into configured agent instruction files as managed blocks.

Rules:

- dalo only creates directory symlinks to resolved store paths.
- dalo only writes instruction text inside its own managed block markers.
- dalo does not replace real directories or files without an explicit action.
- dalo only removes symlinks that it created and knows in `state.toml`.
- dalo only removes instruction blocks that it created and knows in `state.toml`.
- Unmanaged files and directories remain untouched.
- Unmarked instruction-file content remains untouched.
- Broken symlinks are reported by `doctor` and `status`.
- A target may contain only one active managed link per slot name.
- A target instruction file may contain multiple managed instruction blocks, ordered by source priority and pack priority.

Directory symlinks are deliberate. If an agent edits a materialized team skill through any target, it edits the underlying source checkout and makes that source dirty. All enabled targets that link the same resolved skill see the same underlying content. This avoids copy drift, but it also means cloud-synced target folders such as iCloud or Dropbox can be fragile; `doctor` should warn when a target appears to live inside a common cloud-sync location.

If a resolved managed skill wants a slot already occupied by an unmanaged real directory or foreign symlink, `sync` reports a conflict and never touches the existing entry. V1 blocks only that affected target slot where possible and continues materializing safe slots. A later `resolve` flow may offer adoption, explicit replacement, ignore/protect, or a rename/adapt flow, but none of those choices happen automatically.

If a skill is removed from a source, dalo removes its own symlink from targets during the next sync. The actual source content remains available only through Git history or the store checkout.

If an instruction pack is removed from a source, dalo removes only its own managed block from configured instruction files. Manual content outside dalo markers must never be removed.

## 16. Status Model

`status` should be able to show skills in at least these states:

- `managed`: installed by dalo
- `unmanaged`: exists in a target but is not managed by dalo
- `unlinked`: managed skill exists in the store but is not linked into a target
- `local`: from the user's local source
- `team`: from a team source
- `external`: from a pinned external source
- `shadowed`: internal diagnostic/reason for an unlinked skill, caused by another managed source with higher priority
- `blocked_by_same_name_skill`: not linked because an unmanaged, project, or foreign same-name entry occupies the target slot
- `dirty`: source working tree has local changes
- `conflicted`: conflict cannot be resolved safely
- `orphaned`: link or state points to a removed source
- `protected`: `.local` or another protection rule prevents automatic action
- `pending_approval`: a skill is available or selected but not yet approved for local activation
- `new_available`: a catalog source contains a new unselected skill
- `selected_removed`: a selected catalog skill disappeared upstream
- `selected_moved`: a selected catalog skill appears to have moved
- `dependency_missing`: a selected skill declares a dependency that is not selected, available, approved, or linkable
- `dependency_blocked`: a selected skill is not materialized because a required dependency cannot be linked consistently
- `instruction_active`: an instruction pack is active and materialized
- `instruction_topic_overlap`: multiple active instruction packs declare the same topic
- `instruction_block_drift`: a managed instruction block differs from the resolved source content
- `instruction_block_orphaned`: a managed instruction block references a removed instruction pack

Conflicts may be visible without always being blocking. Equal slot names are resolved deterministically by source priority; non-winning managed skills are reported as `unlinked` with reason `shadowed`.

## 17. CLI Surface

All relevant commands should be interactive for humans and stable for automation.

Global flags:

```text
--json       machine-readable output
--yes        non-interactive confirmation for safe actions
--dry-run    show planned actions without changing anything
--store      alternate store path
```

`--yes` must not create Git commits. Commits write source history and require an explicit command, explicit flag, or interactive confirmation in a flow that is already about committing or promoting work.

### 17.1 `dalo init`

Sets up the store, local source, configuration, and optional targets.

Behavior:

- creates `~/.dalo`
- initializes `~/.dalo/local` as a private Git repository
- detects existing agent targets
- detects existing agent instruction files
- shows unmanaged skills
- does not change existing agent files
- does not rewrite existing instruction files
- offers target linking

### 17.2 `dalo target detect`

Detects known agent skill directories, instruction files, and unmanaged skills.

No mutation.

### 17.3 `dalo target link`

Enables a target for materialization.

Examples:

```text
dalo target link codex
dalo target link generic ~/.my-agent/skills
```

### 17.4 `dalo target unlink`

Disables a target. Optionally removes dalo-owned symlinks and managed instruction blocks from that target. Unmanaged files and unmarked instruction content are never removed.

### 17.5 `dalo source add`

Adds a Git or local source.

Examples:

```text
dalo source add company git@github.com:example/company-skills.git
dalo source add oss https://github.com/example/oss-skills.git
dalo source priority oss 20
```

New direct sources must be added explicitly by the user.

Catalog examples (planned, not part of the V1 CLI):

The catalog workflow below — `source add-catalog`, `source inspect`, and
`source select` (including its `--path` flag) — is aspirational and not yet
implemented. The V1 `source` command exposes only `add`, `list`, and
`priority`.

```text
dalo source add-catalog marketing-skills https://github.com/example/marketing-skills.git
dalo source inspect marketing-skills
dalo source select marketing-skills positioning
dalo source select marketing-skills launch-copy --path skills/launch-copy
```

### 17.6 `dalo source list`

Lists sources, priority, trust, sync status, head commit, and dirty state.

### 17.7 `dalo source remove`

Disables or removes a source. On the next sync, only dalo-owned symlinks are removed.

### 17.8 `dalo source priority`

Sets source order. This order decides equal-name skill conflicts.

### 17.9 `dalo status`

Shows:

- active skills
- local skills
- unmanaged skills
- unlinked skills, including whether they are shadowed by a higher-priority managed skill
- same-name target blockers when a managed skill cannot be linked because an unmanaged or project skill occupies the slot
- dirty sources
- conflicted skills
- orphaned symlinks
- protected `.local` skills
- catalog skills that are new, moved, removed, changed, or missing declared dependencies
- skills pending local approval, including source, author/org metadata when available, and the reason approval is needed
- active instruction packs
- instruction topic overlap
- managed instruction block drift
- orphaned managed instruction blocks
- target problems

`status --json` is the foundation for agents, CI, or later UI layers.

### 17.10 `dalo sync`

Refreshes the local skill environment and materializes the resolved asset set.

Normal behavior:

- fetches and fast-forwards only clean tracking team sources
- respects pins for external, catalog, and pinned sources
- respects catalog selections and does not materialize unselected catalog skills
- creates or updates symlinks
- creates or updates managed instruction blocks
- removes only known owned symlinks
- removes only known owned instruction blocks
- reports shadowing
- reports instruction topic overlap
- reports new catalog skills
- reports skills pending approval
- stops on dirty state, broken locks, selected catalog skill removal, instruction block drift during scheduled or non-interactive sync, or unresolved conflicts

`sync` is the everyday command. It is intentionally both the source-refresh step for tracking team sources and the materialization step for agent targets. Scheduler integrations run ordinary `sync` with non-interactive flags such as `--yes --quiet`; there is no separate `sync --auto` mode in the core model.

### 17.11 `dalo instruction list`

Lists available and active instruction packs from all active sources.

### 17.12 `dalo instruction enable`

Enables an instruction pack for materialization.

Example:

```text
dalo instruction enable company.engineering-defaults
```

### 17.13 `dalo instruction disable`

Disables an instruction pack and removes only its owned managed blocks during the next sync.

### 17.14 `dalo adopt`

Adopts unmanaged skills into the local source.

Default flow:

1. detect unmanaged skill
2. copy skill into `~/.dalo/local/skills/<slot-name>`
3. ask whether to commit the copied skill when running interactively
4. offer to replace the original folder with a symlink
5. do not remove existing files without confirmation

Adoption is local-first. When the adopted skill has the same slot name as a team or external skill, the local source wins by priority and the adopted skill becomes a visible local override. Team updates for the shadowed slot remain available in `status`, but they do not replace the local override until the user removes, renames, adapts, or promotes it.

If the user declines the commit prompt, or if `adopt` runs non-interactively without an explicit `--commit`, the local source remains dirty and `status` reports that. `--yes` must not imply `--commit`.

`.local` skills remain protected. Adoption is possible but always explicit.

### 17.15 `dalo promote`

Turns a local skill or dirty team edit into a team contribution.

Vision:

- PR-first workflow
- creates or reuses an explicit commit for the promoted skill changes
- with multiple team sources, `--target` or interactive selection is required
- creates branch, commit, push, and GitHub PR through the `gh` CLI
- requires `gh` to be installed and authenticated
- fails hard when `gh` is missing or not authenticated; no partial local-only promote fallback
- no internal GitHub API client and no secret management in V1
- GitLab and other forges remain future features

Examples:

```text
dalo promote copy-editing --target company
dalo promote copy-editing --from-dirty --target company
```

### 17.16 `dalo resolve`

Explicit tools for resolving known blocking states.

V1 scope:

- `dalo resolve list`: show blocking states with stable IDs and suggested commands
- `dalo resolve adopt <id>`: resolve a managed/unmanaged collision by copying the unmanaged entry into the local source first
- `dalo resolve keep <id>`: keep an unmanaged/protected same-name entry and leave the managed skill blocked for that target slot
- `dalo resolve remove-owned <id>`: remove broken or orphaned dalo-owned symlinks

V1 `resolve` is a small safe toolbox, not a general repair assistant. It does not run a broad "fix everything" flow. Dirty team skills are reported; `promote` is the PR-first path for turning those edits into team changes. Instruction-pack drift recovery belongs to V1.1 because instruction packs themselves are deferred.

Later `resolve` slices may add source restore, lock regeneration, rename/adapt, instruction-block drift recovery, and richer interactive guidance.

### 17.17 `dalo source refresh`

Refreshes pinned source references and creates reviewable lockfile changes.

Default:

- normal `sync` already refreshes clean tracking team sources
- pinned external, catalog, or pinned team sources move only through `source refresh`
- targeted updates by source or skill
- catalog updates compare old and new inventories before changing selected skills
- new unselected catalog skills are reported but not automatically enabled
- removed selected catalog skills block until selection is changed or intentionally removed
- optional GitHub PR for lockfile changes

### 17.18 `dalo doctor`

Checks:

- store structure
- config syntax
- lockfile consistency
- source reachability
- Git auth
- `gh` availability and authentication for PR flows
- target paths
- broken symlinks
- malformed managed instruction block markers
- managed instruction blocks whose source no longer exists
- instruction topic overlap
- unmanaged/protected skills
- pending approvals and the current approval graph
- catalog selections whose paths no longer exist
- selected skills with missing declared dependencies
- catalog inventories that cannot be scanned
- scheduler installation
- unknown targets

### 17.19 `dalo approve`

Approves newly active skill artifacts or the origins that produce them.

Examples:

```text
dalo approve list
dalo approve skill coreyhaines-marketing:launch-copy
dalo approve source coreyhaines-marketing
dalo approve author coreyhaines
dalo approve org company
```

Approval is local user state stored in the dalo store, not in the team repository. Approval scopes:

- `skill`: approves exactly one source-qualified skill, optionally tied to its stable ID and current fingerprint
- `source`: approves future active skills from one source
- `author`: approves future active skills attributed to one author
- `org`: approves future active skills attributed to one organization or owner namespace

Scheduled or non-interactive `sync` never grants approvals. It skips or blocks newly active unapproved skills and reports them through `status` and `doctor`.

## 18. Scheduled Sync

`dalo autosync install` sets up an OS-native scheduler:

- macOS: launchd
- Linux: systemd timer, falling back to cron only when necessary

Default:

- daily
- with jitter
- runs `dalo sync --yes --quiet`
- writes logs into the store
- reports blocking conflicts visibly through `status` and `doctor`

Scheduled sync must not make interactive decisions or overwrite dirty states. `--yes` only confirms safe standard actions; dirty sources, unresolved conflicts, malformed locks, and protected target slots still block.

Scheduled sync never commits local source changes. Dirty local skills remain local working-tree changes until the user explicitly commits, discards, or promotes them.

Blocked scheduled syncs are always recorded in the store log and surfaced through `status` and `doctor`. V1 does not send email, Slack/webhooks, or desktop notifications. OS-level or external notifications can be added later without changing sync semantics.

## 19. Conflict Model

### 19.1 Equal Slot Names

If multiple managed sources provide the same slot name, the source with the highest user priority wins. The others become unlinked with reason `shadowed`. If the local source wins, the winner is additionally shown as a `local override`.

This is not a hard error, but it must be visible:

```text
active: company/copy-editing
unlinked: oss/copy-editing
reason: shadowed by company/copy-editing
```

```text
active: local/copy-editing (local override)
unlinked: company/copy-editing
reason: shadowed by local/copy-editing
```

`shadowed` is the resolver cause. `unlinked` is the user-facing state: the skill exists in the store but is not linked into the target. This distinction keeps the UI close to the actual symlink model.

### 19.2 Managed vs Unmanaged Slot Collision

A managed/unmanaged slot collision happens when the resolver wants to materialize a managed skill into a target slot that already contains an unmanaged real directory or foreign symlink.

V1 behavior:

- report the slot as `conflicted`
- never overwrite, move, delete, or relink the unmanaged entry
- use `blocked_by_same_name_skill` when the blocker appears to be an unmanaged, project, or foreign same-name skill
- materialize other safe slots where possible
- expose the conflict in `status` and `doctor`

Guided resolution is explicit and may include:

- `adopt`: copy the unmanaged folder into the local source first, then optionally replace the target folder with a dalo symlink
- keep/protect unmanaged: leave the existing folder in place and skip the managed skill for that target slot
- explicit replacement: only after confirmation, preserve the unmanaged folder first, then create the managed symlink
- rename/adapt: in a later slice, intentionally create a renamed variant and update machine-readable same-source references where possible

None of these actions happen during scheduled sync.

### 19.2.1 Deferred Rename/Adapt Flow

V1 and V1.1 do not provide a parallel alias layer or runtime router skill for same-name variants. If users later need two same-name variants at the same time, dalo may add an explicit rename/adapt flow.

Example:

```text
dalo adapt-name coreyhaines:review review_coreyhaines
```

Expected semantics:

- create a real local or team-ready copy under the new slot name
- leave the original upstream skill unchanged and updateable
- update machine-readable same-source `requires` references where possible
- warn when text references or ambiguous references cannot be safely updated
- produce a local dirty skill or an explicit PR flow, not a hidden alias

This is a deferred power-user feature. V1 users should resolve same-name variants through source priority, adoption, promotion, or a manual fork.

### 19.3 Dirty Team Skills

If a symlinked team skill is edited locally, the underlying source becomes dirty. Scheduled or non-interactive `sync` blocks for that source or skill.

The user can then choose explicit Git actions:

- `promote --from-dirty`
- `stash`
- `local override`
- `discard`

None of these actions happen automatically.

### 19.4 `.local`

A skill with a `.local` suffix or matching protection marker is private and protected.

Rules:

- never overwrite automatically
- never remove automatically
- report visibly on conflicts
- adoption or promotion only explicitly

`.local` is a legacy and protection marker, not the primary management mechanism. The primary mechanism for private skills is the local source.

### 19.5 Orphans

An orphan exists when a target symlink or state entry points to a source or skill that no longer exists.

Dalo may remove its own orphaned symlinks, but it should make that visible in `status` and `doctor`.

### 19.6 Catalog Drift

Catalog drift happens when an upstream multi-skill repository changes shape between pinned commits.

Rules:

- newly discovered unselected skills are informational
- changed selected skills are handled through normal source-refresh review
- moved selected skills may be reconciled automatically only when stable IDs match
- removed selected skills block scheduled sync for that selection
- missing, unapproved, or unlinked declared dependencies block the dependent selected skill during preflight

### 19.7 Instruction Block Drift

Instruction block drift happens when a managed instruction block in an agent instruction file differs from the resolved instruction pack content.

Rules:

- unmarked content is never considered drift and must not be changed
- interactive sync may offer to restore the managed block from source
- scheduled sync reports and blocks on drift instead of overwriting local edits
- converting local block edits into a local instruction pack is deferred beyond the first instruction-pack release
- orphaned managed blocks may be removed only when their markers are intact and the block is known in `state.toml`

## 20. Security and Trust Model

Dalo treats skills as executable agent instructions with supply-chain relevance.

Principles:

- no custom secret management
- no generic settings sync or dotfile management
- no silent floating updates of external sources
- no silent first activation of new skill artifacts
- no silent rewriting of unmarked agent instruction content
- new direct user sources only through explicit action
- team sources may declare external and catalog sources, but newly active skills still require approval unless covered by an existing approval rule
- lockfile changes are reviewable
- PR flows use existing `gh` authentication
- project repositories must not automatically activate global skills

Approval is modeled separately from source configuration. A source can be configured, reachable, and selected while one or more skills from it remain pending local approval. This mirrors package-manager safety models where newly introduced artifacts with local execution or instruction impact require acknowledgement before they become active.

Approval may be granted at different scopes:

- skill-level approval for one exact source-qualified skill
- source-level approval for future active skills from a configured source
- author-level approval for future active skills attributed to a known author
- org-level approval for future active skills attributed to an organization or owner namespace

When a team source, external source, catalog source, or source refresh introduces a skill that would become active for the first time, dalo checks these approval rules. If no rule applies, the skill is `pending_approval`. Interactive commands may offer to approve the skill or a broader origin. Scheduled sync must not approve anything and must not materialize unapproved skills. If an unapproved higher-priority skill would shadow an already approved lower-priority skill, the approved skill remains active until the new one is approved.

Approval metadata should be stored locally in the dalo store and exposed through `status --json` and `doctor`. At minimum it should record the approval scope, the approved identifier, the granting user or local actor when known, and enough source metadata to explain why a later skill matched the approval.

`doctor` should surface risks, for example:

- source without lock
- external source with floating ref and no pin
- newly active skill pending approval
- broad approval scopes such as source-, author-, or org-level approvals
- selected catalog skill that disappeared upstream
- selected catalog skill with missing declared dependency
- managed instruction block drift
- instruction topic overlap
- malformed managed instruction block markers
- broken symlink
- unmanaged skill with slot-name conflict
- dirty team checkout

## 21. Project-Specific Skills

Real project-specific skills remain in the code repository they belong to. Many agents already discover those project skills by themselves. Dalo should not automatically switch global user targets based on the current project.

Dalo cannot know every future project context, especially for agents that discover nested project skills dynamically based on the current working directory or edited files. When dalo can see a same-name project skill during a concrete target scan, it should report the managed skill as `blocked_by_same_name_skill` with the blocker path. When it cannot see the project context, it makes no global guarantee.

## 22. Promotion Flow

Promotion is the feedback path from local skill growth into team standards.

### 22.1 Local Skill to Team Skill

```text
dalo adopt writing-helper
dalo promote writing-helper --target company
```

Expected behavior:

- target team source is explicit
- skill is validated
- branch is created
- files are copied into the team repository
- commit is created
- GitHub PR is created through `gh`

### 22.2 Dirty Team Edit to PR

```text
dalo status
dalo promote copy-editing --from-dirty --target company
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
- instruction topic overlap
- managed instruction block drift during interactive sync

Blockers:

- missing `SKILL.md`
- broken source path
- newly active skill without matching approval in scheduled or non-interactive sync
- symlink target outside allowed store paths
- unparseable config or lockfiles
- dirty state during scheduled or non-interactive sync
- unresolved conflict in target path
- selected catalog skill removed upstream
- missing, unapproved, or unlinked required dependency in a selected skill's required closure
- malformed managed instruction block markers
- managed instruction block drift during scheduled or non-interactive sync

## 24. Acceptance Criteria

- `init` detects existing agent skill directories, shows unmanaged skills, and does not change them.
- `init` detects existing agent instruction files and does not rewrite them.
- Multiple sources resolve deterministically through user priority.
- The user lock records the resolved asset set; it is prescriptive for pinned sources and descriptive for tracking team sources.
- Instruction packs are resolved as a separate asset type from skills.
- Instruction packs render only into dalo-owned managed blocks.
- Unmarked content in `AGENTS.md`, `CLAUDE.md`, or agent-specific instruction files is never rewritten.
- Topic overlap between instruction packs is reported without semantic inference.
- Scheduled sync blocks on managed instruction block drift.
- Catalog sources allow selecting only specific skills from a multi-skill repository.
- Catalog updates report new upstream skills without enabling them automatically.
- Catalog updates block when a selected skill disappears upstream.
- Catalog updates reconcile moved selected skills only when stable IDs match.
- Same-source and same-catalog required dependencies are expanded transitively; missing, unapproved, or unlinked required dependencies block dependent skills before materialization.
- Same-source relative dependency names are supported as first-class references for external multi-skill repositories.
- Equal managed slot names are reported visibly as active/unlinked, with `shadowed` as the reason.
- `sync` updates clean tracking team sources, respects pinned sources, and blocks dirty states without data loss.
- Newly active skills are not materialized until approved directly or covered by an approved source, author, or org.
- Managed skills are materialized through symlinks.
- Managed/unmanaged slot collisions are reported without touching the unmanaged entry, while other safe slots still materialize where possible.
- Removed skills delete only dalo-owned symlinks, never real files.
- `adopt` copies into the local source first and replaces the original folder only after confirmation.
- `.local` skills remain protected.
- `promote` uses `git` and authenticated `gh` directly to create a PR for local skills or dirty team edits against the explicitly selected team target.
- `doctor` finds broken symlinks, missing auth, broken locks, and unknown targets.
- macOS and Linux are fully supported.

## 25. Open Questions

There are no blocking open product questions for the V1 RFC. Deferred features are tracked in §26.4.

## 26. Suggested V1 Slice

Although this RFC describes the full vision, the first implementation should be narrow. V1 proves the architecture — store, resolver, materialization, reconciliation — on the core single- and multi-source skill loop, before taking on the features that carry the most unresolved design questions.

### 26.1 V1 scope

- `init`
- `target detect/link/unlink`
- `source add/list/priority` for `local` and `team` sources
- `status`
- `sync`
- `adopt`
- minimal `resolve` toolbox for listed blockers, adoption into local source, keep/protect, and owned symlink cleanup
- `doctor`
- TOML config
- local store
- symlink materialization
- deterministic multi-source resolution
- user lockfile for the resolved asset set
- local approval state for newly active team skills
- warnings for unlinked shadowed skills and dirty state

### 26.2 Deferred to V1.1

Catalog support and instruction packs are deferred to V1.1, not because they are unimportant, but because they add schema and UX surface that should not block the first slice: catalog inventory and move-detection rules, source-refresh review flow, explicit instruction-pack enablement, and target-file mapping (§13). Pulling them out of V1 keeps the first slice focused on the riskiest engine code.

V1 still materializes skills and runs `adopt`, so it must already handle managed/unmanaged slot collisions. V1 applies the conservative rule guaranteed in §15 and RFC 0003 D3: detect the collision, report it, never overwrite an unmanaged entry, and materialize other safe slots where possible. Guided resolution is explicit and may include adoption, keep/protect, replacement after preservation and confirmation, or rename/adapt in a later slice. Slot collision is therefore not a reason to defer catalog or instruction packs.

- basic catalog source inspection and explicit selection
- lockfile entries for selected catalog skills
- warnings for new catalog skills and preflight blockers for missing declared dependencies
- basic instruction pack discovery and managed block rendering
- no dependency on native include/import support for instruction packs
- warnings for instruction topic overlap

### 26.3 Suggested implementation sequence

The V1 scope is a feature set, not an order. A workable sequence:

1. store layout, TOML config parsing, and `init`
2. target registry and `target detect/link/unlink`
3. inventory scan and single-source `sync` with symlink materialization
4. local approval checks for newly active skills
5. deterministic multi-source resolution (priority, shadowing) and `status`
6. `adopt` into the local source, with optional symlink replacement
7. `doctor` diagnostics

This sequence front-loads the store and materializer (the highest-risk filesystem code) and adds multi-source behavior only once a single source materializes safely.

### 26.4 Not in early slices

- full automatic scheduler
- `source refresh` with lockfile PRs
- full interactive `resolve` assistant
- explicit rename/adapt flow for keeping same-name variants in parallel
- automatic catalog move reconciliation
- cross-source automatic dependency installation
- advanced instruction block drift recovery
- full `promote` PR flow
- all agent targets
- forge adapters beyond GitHub

The early slices should prove the architecture without implementing the entire vision at once.

# Skillmgr

Git-backed skill management for AI agents.

Skillmgr gives teams one place to manage the skills, shared agent conventions, and local experiments their AI agents depend on. It keeps the source of truth in a local store, resolves skills from multiple Git sources, and links the final set into the agent folders people already use.

Codex, Claude Code, OpenClaw, Hermes, and folder-based agents can keep reading normal skill directories. Skillmgr handles the part that gets hard once skills become team knowledge: source priority, safe sync, local overrides, catalog selection, adoption, promotion, and drift.

## Why this exists

Agent skills are becoming operational knowledge.

They encode how your team reviews code, writes release notes, investigates incidents, uses internal tooling, formats output, and applies judgment that should not live in one developer's private folder.

Most existing flows are still built around one machine:

- install this skill here
- copy that folder there
- choose the target agent again
- hope nobody changed the public repo
- hope the local edit does not get lost
- hope the team knows which version is in use

That is fine for experimenting. It breaks down when agents become part of daily engineering work.

Skillmgr treats agent knowledge like something worth managing:

- versioned in Git
- resolved deterministically
- synced across a team
- kept separate from private local work
- safe around existing unmanaged files
- ready to promote through review when a local experiment proves useful

## The model

Skillmgr separates source, resolution, and installation.

```text
team sources + local source + selected catalogs
        |
        v
~/.skillmgr store
        |
        v
deterministic resolver
        |
        v
agent skill folders + managed instruction blocks
```

The agent folder is an output target, not the database.

That one boundary keeps the system predictable. Team repositories stay clean Git sources. Local experiments live in a private local source. Agent folders contain symlinks and remain safe to inspect. Existing files are never silently absorbed or overwritten.

## What it feels like

Set up the store, detect agents, link the targets you want, add a team source, then sync.

```sh
skillmgr init
skillmgr target detect
skillmgr target link codex
skillmgr target link claude
skillmgr source add company git@github.com:acme/agent-skills.git
skillmgr sync
```

The team skills appear where each agent expects them.

When an agent creates a useful local skill directly in its own folder, Skillmgr can bring it under management without losing the original work.

```sh
skillmgr status
skillmgr adopt release-notes.local
```

The first step copies the skill into the local source. Replacing the original folder with a symlink is a separate confirmation. Committing the adopted skill is also explicit; `--yes` never means "commit this for me".

When that local skill is ready for the team, the intended full flow is PR-first:

```sh
skillmgr promote release-notes --target company
```

Promotion uses normal `git` plus an authenticated `gh` CLI. If GitHub auth is missing, Skillmgr should fail loudly instead of inventing a weaker fallback.

## Sources

Skillmgr is multi-source by default.

| Source kind | Purpose | Version behavior |
| --- | --- | --- |
| Local | Private user skills and instruction packs | Local Git repository in the store |
| Team | Shared team skills and conventions | Usually tracks a branch during `sync` |
| External | Git source declared by another trusted source | Pinned through source locks |
| Catalog | Multi-skill repository used as an offer surface | Selected skills are pinned and tracked |

The first V1 implementation starts with local and team sources. External sources and catalogs are the next product layer once the store, resolver, and materializer are proven.

The everyday command is `sync`: refresh clean tracking team sources, resolve the final skill set, and materialize it into configured targets.

Pinned external and catalog sources use explicit lock advancement through `source refresh`. That is a reviewable source-maintenance operation, not the normal daily installation path, and it belongs to the later source-maintenance slice.

## Resolution

Source order decides conflicts. Lower priority wins.

If two sources provide the same skill slot, Skillmgr links the highest-priority approved skill and reports the others as `unlinked` with reason `shadowed`.

If a target folder already contains a real unmanaged skill with the same name, Skillmgr does not overwrite it. The managed skill is blocked with `blocked_by_same_name_skill`, and `status` explains what happened.

The important rule is simple:

> Skillmgr may remove or repair what it owns. It does not take ownership of user files by surprise.

## Public skill catalogs

Some of the most useful public repositories are not one-skill packages. They are catalogs: one Git repository with many skills inside, often with informal relationships between them.

Skillmgr is designed to treat those repositories as offer surfaces. This is a V1.1 feature, not the first implementation slice.

You inspect the catalog, select the skills you want, and Skillmgr records the selected set in a lockfile. It also scans the full catalog inventory so it can detect meaningful upstream changes:

- new skills are available
- selected skills changed
- selected skills moved
- selected skills disappeared
- selected skills now require other same-catalog skills

Same-source and same-catalog `requires` are first-class. If you select a skill that depends on another skill from the same catalog, Skillmgr expands that required closure. If a required skill is missing, unapproved, unlinked, or blocked by a name conflict, the dependent skill blocks before materialization.

Cross-source dependencies are checked, not auto-installed.

## Instruction packs

Some shared agent context is not a skill.

Examples:

```markdown
Use OXLint for linting and OXFMT for formatting.
Prefer TypeScript for application code.
Use Rust for performance-sensitive non-browser code.
Call out behavioral regressions before style nits.
```

Skillmgr models this as an instruction pack: a versioned Markdown artifact rendered into agent instruction files as a managed block. This is also a V1.1 feature.

```markdown
<!-- skillmgr:start company.engineering-defaults -->
Use OXLint for linting and OXFMT for formatting.
Prefer TypeScript for application code.
<!-- skillmgr:end company.engineering-defaults -->
```

Everything outside the marked block belongs to the user or project. Skillmgr does not manage arbitrary dotfiles, editor settings, formatter configs, shell profiles, secrets, or project-specific instructions.

Native include/import support is not portable enough to be the baseline. Managed blocks are the conservative V1.1 target.

## Target agents

V1 focuses on directory-based skill targets.

| Agent | Default skill path | Status |
| --- | --- | --- |
| Codex | `~/.agents/skills` | V1 target |
| Claude Code | `~/.claude/skills` | V1 target |
| OpenClaw | `~/.agents/skills` | V1 target |
| Hermes | `~/.hermes/skills` | V1 target |
| Generic folder target | User-provided path | V1 target |
| Cursor | unverified | experimental |
| OpenCode | unverified | experimental |

When multiple agents share the same physical directory, Skillmgr should de-duplicate the target path and avoid double-linking the same slot.

## Safety guarantees

Skillmgr is intentionally conservative.

It should never:

- delete unmanaged files
- overwrite a real skill folder with a symlink without confirmation
- rewrite unmarked instruction content
- silently enable newly active skills during scheduled sync
- advance pinned external sources without a lockfile change
- overwrite dirty team checkouts during non-interactive sync
- commit adopted local work just because `--yes` was passed

Newly active skills require local approval unless covered by a trusted source, author, or organization approval. Scheduled and non-interactive sync can apply existing approvals, but they never grant new ones.

Dirty team edits block sync for the affected source or skill. The guided answer is promotion, stash, local override, or discard. The default is no data loss.

## Compared with local installers

Single-user skill installers are useful. They are good at putting a skill onto one machine.

Skillmgr starts at the point where that workflow gets awkward for teams.

| Approach | Good fit | Limit |
| --- | --- | --- |
| One-off skill installer | Personal setup | Repeated target choices, weak team review, unclear versions |
| Dotfile repo | Personal environment | Too broad, easy to overwrite unrelated files |
| Git submodule | Pinning one external repo | Awkward selective use, detached UX, poor catalog fit |
| Copying public folders | Fast experiments | No drift detection, provenance, or upgrade path |
| Project-local instructions | Repo-specific context | Does not solve global team agent behavior |

Skillmgr is narrower than a dotfile manager and more team-aware than a local installer.

It is the missing layer between "I found a useful skill" and "our agents can rely on this."

## V1 scope

The first implementation should prove the core loop before expanding the surface area.

V1 includes:

- `init`
- `target detect`, `target link`, `target unlink`
- `source add`, `source list`, `source priority` for local and team sources
- `status`
- `sync`
- `adopt`
- minimal `resolve` helpers for listed blockers
- `doctor`
- TOML config and lockfiles
- local store under `~/.skillmgr`
- directory-level symlink materialization
- deterministic multi-source resolution
- local approval state for newly active team skills
- user-facing `unlinked` reporting for shadowed skills
- dirty-state blocking for non-interactive sync

V1.1 adds the next product layer:

- catalog inspection and selected catalog locks
- catalog drift reporting
- same-catalog required-closure expansion
- instruction pack discovery and managed block rendering
- topic-overlap warnings for instruction packs

Later work includes:

- full scheduled autosync installation
- `source refresh` lockfile PRs
- full interactive resolve assistant
- rename/adapt flows for conflicts
- automatic catalog move reconciliation
- full PR-first promotion
- additional verified agent adapters
- forge adapters beyond GitHub

## Development

Skillmgr is implemented as a Rust CLI with a reusable library core.

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --help
```

The current implementation has the Rust project scaffold, CLI shell, store layout, TOML schemas, `skillmgr init`, target detect/link/unlink, deterministic skill inventory scanning, resolver, approval gating, and `skillmgr status` in place. Product behavior is being implemented milestone by milestone from [the implementation plan](docs/milestones/README.md).

## Project status

Skillmgr is currently in early implementation.

The product direction, resolver model, source terminology, target strategy, approval model, and V1/V1.1 boundary are defined in RFCs. The README describes the intended product, while the RFCs and milestone plan define the implementation contract.

## Design docs

- [Implementation milestones](docs/milestones/README.md)
- [RFC 0001: Skillmgr vision](docs/rfcs/0001-skillmgr-vision.md)
- [RFC 0002: Technical architecture](docs/rfcs/0002-technical-architecture.md)
- [RFC 0003: Resolution engine](docs/rfcs/0003-resolution-engine.md)
- [ADR 0001: Project language](docs/adr/0001-project-language.md)

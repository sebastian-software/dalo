# Dalo

Git-backed skill management for AI agents.

Dalo gives teams one place to manage the skills and local experiments their AI agents depend on. It keeps the source of truth in a local store, resolves skills from multiple Git sources, and links the final set into the agent folders people already use.

Codex, Claude Code, OpenClaw, Hermes, and folder-based agents can keep reading normal skill directories. Dalo handles the part that gets hard once skills become team knowledge: source priority, safe sync, local overrides, adoption, diagnostics, and drift.

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

Dalo treats agent knowledge like something worth managing:

- versioned in Git
- resolved deterministically
- synced across a team
- kept separate from private local work
- safe around existing unmanaged files
- ready to promote through review when a local experiment proves useful

## Installation

Dalo currently targets Unix-like systems (Linux and macOS). Windows is not yet supported.

The current release candidate is source-built:

```sh
git clone https://github.com/sebastian-software/dalo.git
cd dalo
cargo install --path .
```

For development:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
```

Dalo shells out to `git` for source operations. `gh` is checked by `doctor` for future PR flows, but normal V1 sync does not require GitHub auth.

## Quickstart

This quickstart uses a temporary store and a temporary generic target, so it will not touch your real agent folders.

```sh
export DALO_STORE="$(mktemp -d)/store"
export DALO_TARGET="$(mktemp -d)/skills"

dalo init
dalo target link generic "$DALO_TARGET"

mkdir -p "$DALO_STORE/local/skills/review"
printf '# Review\n' > "$DALO_STORE/local/skills/review/SKILL.md"

dalo status
dalo sync
ls -la "$DALO_TARGET"
dalo doctor
```

To try a local team source without network access:

```sh
TEAM_REPO="$(mktemp -d)/team-skills"
mkdir -p "$TEAM_REPO/skills/release-notes"
printf '# Release Notes\n' > "$TEAM_REPO/skills/release-notes/SKILL.md"
git -C "$TEAM_REPO" init
git -C "$TEAM_REPO" add .
git -C "$TEAM_REPO" -c user.email=test@example.com -c user.name='Test User' commit -m initial

dalo source add company "$TEAM_REPO"
dalo source list
dalo sync
```

## The model

Dalo separates source, resolution, and installation.

```text
team sources + local source
        |
        v
~/.dalo store
        |
        v
deterministic resolver
        |
        v
agent skill folders
```

The agent folder is an output target, not the database.

That one boundary keeps the system predictable. Team repositories stay clean Git sources. Local experiments live in a private local source. Agent folders contain symlinks and remain safe to inspect. Existing files are never silently absorbed or overwritten.

## What it feels like

Set up the store, detect agents, link the targets you want, add a team source, then sync.

```sh
dalo init
dalo target detect
dalo target link codex
dalo target link claude
dalo source add company git@github.com:acme/agent-skills.git
dalo sync
```

The team skills appear where each agent expects them. A source explicitly added by the user is locally approved for V1 resolution.

When an agent creates a useful local skill directly in its own folder, Dalo can bring it under management without losing the original work.

```sh
dalo status
dalo adopt release-notes.local
```

The first step copies the skill into the local source. Replacing the original folder with a symlink is a separate confirmation. Committing the adopted skill is also explicit; `--yes` never means "commit this for me".

Promotion is intentionally deferred. The planned flow is PR-first and will use normal `git` plus an authenticated `gh` CLI. If GitHub auth is missing, Dalo should fail loudly instead of inventing a weaker fallback.

## Sources

Dalo is multi-source by default. V1 implements local and team sources.

| Source kind | Purpose | Version behavior |
| --- | --- | --- |
| Local | Private user skills and instruction packs | Local Git repository in the store |
| Team | Shared team skills and conventions | Usually tracks a branch during `sync` |
| External | Git source declared by another trusted source | Pinned through source locks |
| Catalog | Multi-skill repository used as an offer surface | Selected skills are pinned and tracked |

V1 ships local and team sources. V1.1 adds catalog sources (multi-skill repositories used as offer surfaces); external sources remain a later product layer.

The everyday command is `sync`: refresh clean tracking team sources, resolve the final skill set, and materialize it into configured targets.

Catalog sources are drift-checked read-only through `source refresh --check`, which compares the upstream inventory against the pinned snapshot without advancing the pin. Lock advancement (writing a new pin) is a later source-maintenance slice.

## Resolution

Source order decides conflicts. Lower priority wins.

If two sources provide the same skill slot, Dalo links the highest-priority approved skill and reports the others as `unlinked` with reason `shadowed`.

If a target folder already contains a real unmanaged skill with the same name, Dalo does not overwrite it. The managed skill is blocked with `blocked_by_same_name_skill`, and `status` explains what happened.

The important rule is simple:

> Dalo may remove or repair what it owns. It does not take ownership of user files by surprise.

## Public skill catalogs

Some of the most useful public repositories are not one-skill packages. They are catalogs: one Git repository with many skills inside, often with informal relationships between them.

Dalo treats those repositories as offer surfaces (V1.1). Add one with `source add-catalog <id> <url>`, list its skills with `source inspect <id>`, and choose what you want with `source select <id> <skill...>`.

You inspect the catalog, select the skills you want, and Dalo records the selected set in a source lockfile. It also scans the full catalog inventory so `source refresh --check` can detect meaningful upstream changes:

- new skills are available
- selected skills changed
- selected skills moved
- selected skills disappeared
- selected skills now require other same-catalog skills

Same-source and same-catalog `requires` are first-class. If you select a skill that depends on another skill from the same catalog, Dalo expands that required closure. If a required skill is missing, unapproved, unlinked, or blocked by a name conflict, the dependent skill blocks before materialization.

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

Dalo models this as an instruction pack: a versioned Markdown artifact rendered into agent instruction files as a managed block (V1.1). Enable one with `instructions enable <pack> <file>` and remove it with `instructions disable <pack> <file>`; `instructions list` shows what is active.

```markdown
<!-- dalo:start company.engineering-defaults -->
Use OXLint for linting and OXFMT for formatting.
Prefer TypeScript for application code.
<!-- dalo:end company.engineering-defaults -->
```

Everything outside the marked block belongs to the user or project. Dalo does not manage arbitrary dotfiles, editor settings, formatter configs, shell profiles, secrets, or project-specific instructions.

Native include/import support is not portable enough to be the baseline, so managed blocks are the approach. Discovery lists available and enabled packs, and overlapping declared topics across active packs raise an advisory warning in `status` and `doctor`.

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

When multiple agents share the same physical directory, Dalo should de-duplicate the target path and avoid double-linking the same slot.

## Safety guarantees

Dalo is intentionally conservative.

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

Dalo starts at the point where that workflow gets awkward for teams.

| Approach | Good fit | Limit |
| --- | --- | --- |
| One-off skill installer | Personal setup | Repeated target choices, weak team review, unclear versions |
| Dotfile repo | Personal environment | Too broad, easy to overwrite unrelated files |
| Git submodule | Pinning one external repo | Awkward selective use, detached UX, poor catalog fit |
| Copying public folders | Fast experiments | No drift detection, provenance, or upgrade path |
| Project-local instructions | Repo-specific context | Does not solve global team agent behavior |

Dalo is narrower than a dotfile manager and more team-aware than a local installer.

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
- local store under `~/.dalo`
- directory-level symlink materialization
- deterministic multi-source resolution
- local approval state for newly active team skills
- user-facing `unlinked` reporting for shadowed skills
- dirty-state blocking for non-interactive sync

V1.1 adds the next product layer:

- catalog sources: `source add-catalog`, `source inspect`, `source select`
- catalog drift reporting via read-only `source refresh --check`
- same-catalog required-closure expansion with a linkability preflight
- instruction packs: `instructions enable`/`disable`/`list` rendering managed blocks
- instruction pack discovery and topic-overlap warnings

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

Dalo is implemented as a Rust CLI with a reusable library core.

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --help
```

The current implementation has the Rust project scaffold, CLI shell, store layout, TOML schemas, `dalo init`, target detect/link/unlink, team source add/list/priority, deterministic skill inventory scanning, resolver, approval gating, `dalo status`, `dalo sync` symlink materialization, clean team source refresh, dirty-source blocking, a coarse store lock, resolved user-lock writing, status lock-drift reporting, unmanaged skill discovery, copy-first adoption, minimal resolve helpers, and `dalo doctor` diagnostics in place. The V1 release-candidate scope is tracked in [the implementation plan](docs/milestones/README.md).

## Project status

Dalo is currently a V1 release candidate.

The implemented V1 loop covers local/team sources, target linking, safe sync, lock drift, adoption, minimal resolve helpers, and doctor diagnostics. Catalogs, instruction packs, autosync, promotion, and package distribution are deferred.

## Design docs

- [Implementation milestones](docs/milestones/README.md)
- [RFC 0001: Dalo vision](docs/rfcs/0001-dalo-vision.md)
- [RFC 0002: Technical architecture](docs/rfcs/0002-technical-architecture.md)
- [RFC 0003: Resolution engine](docs/rfcs/0003-resolution-engine.md)
- [V1 implementation status](docs/rfcs/v1-implementation-status.md)
- [v0.1.0-rc.1 release notes](docs/releases/v0.1.0-rc.1.md)
- [ADR 0001: Project language](docs/adr/0001-project-language.md)

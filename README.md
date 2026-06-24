# Skillmgr

Your agents are getting better. Their skills should not live in random folders.

Skillmgr is a CLI-first manager for AI-agent skills, team conventions, and shared instruction packs. It gives teams a clean way to version, review, sync, and promote the knowledge their agents rely on.

One developer writes a useful skill. Another improves it. A team turns it into a shared standard. Everyone's agents pick it up safely.

That is the loop Skillmgr is built for.

## Why this should exist

Agent skills are starting to look like code, but most teams still manage them like snippets.

They sit in global folders. They get copied between machines. They drift across agents. Someone installs a useful public skill and nobody knows which version is in use. A local experiment works well for one person, then dies in a private directory because sharing it is too annoying.

That is fine for tinkering. It breaks down once a team starts depending on agents for real work.

Skillmgr treats agent knowledge as part of the engineering system:

- versioned
- reviewable
- reproducible
- local when it should stay local
- shared when it is ready for the team

## What Skillmgr manages

Skillmgr manages two kinds of agent assets.

**Skills** are reusable capabilities with a `SKILL.md` entry point and optional supporting files.

**Instruction packs** are shared standing instructions for agents: team conventions, review style, engineering defaults, writing preferences, and other guidance that belongs in `AGENTS.md`, `CLAUDE.md`, or an agent-specific instruction file.

Skillmgr does not manage arbitrary dotfiles. It does not sync editor settings, shell profiles, linter configs, formatter configs, or secrets. That line matters.

## The core idea

Skillmgr keeps its own local store, resolves the assets you want from multiple sources, and materializes them into the agents you use.

```text
team sources + public catalogs + local skills
        |
        v
skillmgr resolver
        |
        v
one resolved asset set
        |
        v
agent skill folders + managed instruction blocks
```

Agent folders are output targets, not the database.

That one decision avoids a lot of pain. Agents can keep reading from their normal locations, but team state lives in a controlled store where Git, lockfiles, sync, and promotion make sense.

## Built for teams, not just installs

Skillmgr is for teams that want their agents to inherit taste, process, and hard-won lessons.

Use it to:

- keep shared team skills in Git
- pin public skill collections to known commits
- select only the skills you want from a multi-skill repository
- sync Codex, Claude Code, OpenClaw, Hermes, and generic folder-based agents, with room for experimental Cursor and OpenCode adapters
- keep private user skills separate from team skills
- adopt skills that agents created directly in their own folders
- promote good local skills into a team repository through a PR-first workflow
- ship team conventions as managed instruction blocks without touching the rest of a user's files

The goal is not to make every agent identical. The goal is to make the shared parts intentional.

## How it feels

```bash
skillmgr init
skillmgr source add company git@github.com:acme/agent-skills.git
skillmgr target link codex
skillmgr target link claude
skillmgr sync
```

Your team skills appear where your agents expect them.

Then an agent creates something useful in a local skill folder:

```bash
skillmgr status
skillmgr adopt release-notes.local
skillmgr promote release-notes --target company
```

The private experiment becomes a reviewed team asset. No copy-paste ritual. No "which folder was that in again?"

## Multi-source by default

Most teams will not have one perfect source of truth.

You may have:

- company skills
- personal skills
- open-source skill collections
- vendor-maintained skill repositories
- team instruction packs
- local experiments

Skillmgr resolves them in priority order and shows what happened.

If two sources provide `copy-editing`, one wins and the other is shown as unlinked because it is shadowed by the higher-priority source. If a local skill overrides a team skill, that is visible. If a selected public skill disappears upstream, scheduled sync stops instead of silently removing the skill from your agents.

Quiet magic is where trust goes to die. Skillmgr should make the state obvious.

## Public catalogs without the mess

Some of the most useful skill repositories are catalogs: one Git repo with many skills inside.

Skillmgr treats those as offer surfaces. You can inspect the catalog, choose the exact skills you want, and pin the selected set.

```bash
skillmgr source add-catalog marketing-skills https://github.com/example/marketing-skills.git
skillmgr source inspect marketing-skills
skillmgr source select marketing-skills positioning
skillmgr source select marketing-skills launch-copy
skillmgr sync
```

When the upstream catalog changes, Skillmgr tells you what changed:

- new skills are available
- selected skills changed
- selected skills moved
- selected skills disappeared
- selected skills now require other skills

New upstream skills are not enabled behind your back. Removed selected skills block scheduled sync until someone makes a decision.

## Instruction packs, not settings sync

Teams often need to tell agents how work is done here.

For example:

```markdown
Use OXLint for linting and OXFMT for formatting.
Prefer TypeScript for application code.
Use Rust for performance-sensitive non-browser code.
Call out behavioral regressions before style nits.
```

That is not a skill. It is standing context.

Skillmgr manages that as an instruction pack and renders it into agent instruction files as a marked block:

```markdown
<!-- skillmgr:start company.engineering-defaults -->
Use OXLint for linting and OXFMT for formatting.
Prefer TypeScript for application code.
<!-- skillmgr:end company.engineering-defaults -->
```

Everything outside the block belongs to the user or project. Skillmgr does not touch it.

Native include/import support differs too much between agents to be the core contract. Skillmgr's V1.1 instruction-pack slice should use managed blocks as the portable baseline and only use native includes later for targets where the adapter has verified support.

## Where other approaches stop

There are useful tools for installing skills into a local agent setup. They are good for getting something onto one machine.

Skillmgr starts where that workflow gets uncomfortable:

| Approach | Works well for | Where it gets painful |
| --- | --- | --- |
| One-off skill installers | Personal setup | Repeated choices, weak team review, unclear versions |
| Dotfile repositories | Personal config | Too broad, too easy to overwrite unrelated files |
| Git submodules | Pinning external repos | Awkward UX, detached states, poor fit for selective skills |
| Copying public skill folders | Quick experiments | No upgrade path, no drift detection, no provenance |
| Project-local instructions | Repo-specific context | Does not solve shared team-wide agent behavior |

Skillmgr is narrower than a dotfile manager and more team-aware than a local installer.

It is the missing layer between "I found a useful skill" and "our agents can rely on this."

## Safety model

Skillmgr is conservative because it operates in places people care about.

It should never:

- delete unmanaged files
- rewrite unmarked instruction content
- silently enable newly active skills
- float external dependencies without a lockfile
- overwrite dirty team checkouts during scheduled sync
- replace a real folder with a symlink without explicit confirmation

When in doubt, it stops and explains what needs a human decision.

## Designed for the agent era

The more you use agents, the more your actual advantage moves into the instructions, workflows, and judgments they carry.

Your best review checklist. Your release-note style. Your migration playbook. Your team's defaults. The tiny procedural details that make one agent run feel sharp and another feel like a stranger.

Those things deserve a lifecycle.

Draft them locally. Try them in real work. Promote the ones that hold up. Keep the team current. Retire what stops working.

Skillmgr makes that loop explicit.

## Current design docs

The implementation is being shaped through RFCs and ADRs:

- [RFC 0001: Skillmgr vision](docs/rfcs/0001-skillmgr-vision.md)
- [RFC 0002: Technical architecture](docs/rfcs/0002-technical-architecture.md)
- [RFC 0003: Resolution engine](docs/rfcs/0003-resolution-engine.md)
- [ADR 0001: Project language](docs/adr/0001-project-language.md)

The README describes the product bar we are building toward.

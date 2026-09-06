# ADR 0002: Store-Centered Skill Management

Status: Accepted  
Date: 2026-09-06  
Source: [RFC 0001: Dalo Vision](../rfcs/0001-dalo-vision.md)  

## Context

Agent skills are operational knowledge that teams copy between machines and
agent folders by hand. Every agent reads its own directory, so the same skill
diverges per machine, local improvements get lost, upstream changes arrive
unseen, and nobody can say what an agent is actually running.

## Decision

Dalo manages skills, instruction packs, and agents in a local store at
`~/.dalo`, and treats agent directories as output targets.

- Multiple Git-backed sources (local, team, catalog) are active at the same
  time, each with an explicit priority.
- The store owns sources, locks, approvals, and audits; agent directories
  contain only the links Dalo owns.
- Materialization is a symlink into the agent's normal skill directory, so
  agents keep reading the folders they already understand.
- Existing unmanaged files stay untouched; adoption into a source is explicit.
- Dalo is not a registry service and not a project-profile switcher.

## Consequences

- The resolved set is reproducible from the store plus its lockfiles, on every
  machine that shares the same sources.
- Conflicts, drift, and pending approvals are reportable states rather than
  silent overwrites.
- Anything an agent needs must be modeled as a store asset before it can be
  delivered, which keeps the asset vocabulary small and explicit.

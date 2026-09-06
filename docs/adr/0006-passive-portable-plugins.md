# ADR 0006: Passive Portable Plugins

Status: Accepted  
Date: 2026-09-06  
Source: [RFC 0005: Portable Plugins and Composable Agent Stacks Beyond Skills](../rfcs/0005-portable-plugins-and-agent-stacks.md)  

## Context

Skills, instruction packs, and agents are resolved one asset at a time, but
teams ship them as a set that also carries local tools, lifecycle hooks,
dependencies, and fallbacks. Each harness has its own plugin installation
format, and adopting one of them as the canonical format would tie Dalo's
vocabulary to a single vendor and to implicit execution of newly fetched code.

## Decision

A source can group assets in an inert plugin package at
`plugins/<name>/PLUGIN.toml`, and harness plugin formats are projections of it.

- A plugin declares members (skills, agents, instruction packs), tools, and
  hooks with explicit requirement levels, capabilities, and fallbacks.
- Selection expresses intent only. It never grants a skill or agent approval,
  never enables an instruction pack, and never runs anything.
- Plugins resolve through the existing source, lock, approval, audit, and
  ownership model; every projection is previewable with `dalo plan` before any
  provider state is written.
- `dalo plugin review` walks the selected plugin and its dependency closure in
  one session, asks separately for each pending skill, agent, tool, and hook
  contract, and commits that explicit set through one atomic approval entry.
- Manifests reject unknown fields, so an unsupported capability fails loudly
  instead of being ignored.

## Consequences

- Authored intent survives across harnesses without promising lossless
  cross-harness behavior.
- Adding a plugin is never sufficient to execute new code; approval stays a
  separate, auditable step.
- Plugin schema growth is a versioned, documented contract in
  [`docs/reference.md`](../reference.md), not an implicit extension point.

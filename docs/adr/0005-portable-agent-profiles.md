# ADR 0005: Portable Agent Profiles

Status: Accepted  
Date: 2026-09-06  
Source: [RFC 0004: Portable Agent Profiles](../rfcs/0004-portable-agent-profiles.md)  

## Context

Skills package reusable knowledge; a delegated task also needs a role with a
system prompt, model intent, required skills, and boundaries. Every harness has
its own agent format, and those formats are not interchangeable in model
selection, tools, permissions, or hooks.

## Decision

Agents are a managed asset type alongside skills and instruction packs.

- The canonical, provider-neutral package is `agents/<name>/AGENT.md` in a
  source, discovered from local, team, and catalog sources like any other asset.
- Dalo compiles the canonical package into each linked provider's native format
  rather than treating one provider's format as the source of truth.
- Compatibility is reported field by field, and a provider projection is refused
  when it cannot preserve an explicit safety boundary.
- Agents carry their own approvals and audits, and existing provider agents are
  adopted explicitly.
- Materialized agent files are owned regular files written atomically, not
  symlinks.

## Consequences

- One reviewed role definition can serve several harnesses, with the loss of
  fidelity named instead of hidden.
- Provider support grows by adding adapters; the canonical format stays stable.
- Portability is a claim about the prompt-and-metadata shape only, never about
  identical enforcement between harnesses.

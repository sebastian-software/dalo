# ADR 0004: Deterministic Resolution and Safe Materialization

Status: Accepted  
Date: 2026-09-06  
Source: [RFC 0003: Resolution Engine](../rfcs/0003-resolution-engine.md)  

## Context

Several sources can offer the same skill name, agents and scheduled runs write
into the store concurrently, and the filesystem can drift away from what Dalo
recorded. Without one specified engine, `status`, `sync`, and `doctor` would
each answer these questions differently.

## Decision

Resolution is a pure function, and materialization reconciles three states.

- The resolver takes inventories, configuration, and approval state and returns
  a resolution plus typed diagnostics. It performs no filesystem or network
  access and does not decide exit codes.
- The conflict key is the skill slot name, taken from `SKILL.md` frontmatter
  `name` when valid and otherwise from the folder name; references are
  `<source-id>:<slot-name>`.
- Sources are ordered by ascending priority and then by source ID; the winner is
  linked, and every other candidate stays visible as shadowed.
- Required skills are expanded within the same source or catalog before
  selection is final, which is why the resolver receives full catalog
  inventories.
- A managed skill never overwrites an unmanaged directory or a foreign symlink.
  Such a collision is a reported conflict, and the remaining safe slots are
  still materialized.
- A store lock serializes concurrent invocations, and materialization plans are
  computed from desired set, recorded state, and actual filesystem before any
  write.

## Consequences

- The same inputs produce the same resolved set, which makes the engine
  testable without a filesystem and reproducible across machines.
- Users always get a next command instead of a surprise: shadowing, drift,
  pending approval, and blocked slots are enumerable diagnostics.
- New behavior belongs in diagnostics and in the reconciliation plan, not in ad
  hoc printing inside commands.

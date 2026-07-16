# M17: Team Catalog Composition

Status: done
Target: post-V1.1
Depends on: M06, M11-M13, RFC 0001

## Goal

Let a team repository define one reproducible skill environment from its own
checked-in skills plus pinned external multi-skill repositories. Keep source
composition under team review without turning team trust into automatic trust
of third-party skills.

## Delivered

- Root-level `dalo.toml` discovery in enabled team sources.
- Store-independent `dalo team` authoring commands for manifest initialization,
  catalog add/remove, filter replacement, version updates, and inspection.
- `[[catalog]]` declarations with URL, Git version, optional priority, and
  skill filters.
- Derived catalog IDs namespaced as `<team-id>.<catalog-id>`.
- Exact commit persistence in the existing source lock and detached checkout.
- Filter semantics:
  - missing or empty `skills` means all
  - minus-only lists blacklist from all
  - any plus or bare entry activates whitelist mode
  - exclusions always win
- Reconciliation during `sync`, after tracking team sources refresh.
- Removal of stale derived config, lock, and approval records when a declaration
  disappears.
- Local approval and deterministic security-audit boundaries for every derived
  catalog.
- Mutation guards that direct users back to the owning team manifest.

## Validation

- Unit tests cover empty, blacklist, whitelist, and exclusion precedence.
- End-to-end tests cover team-local skills, a historical catalog pin, local
  approval, filter changes, version advancement, and target reconciliation.
- The full Rust test suite passes without network access.

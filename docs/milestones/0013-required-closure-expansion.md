# M13: Same-Catalog Required-Closure Expansion

Status: todo
Target: V1.1  
Depends on: M04, M11, RFC 0001, RFC 0003  

## Goal

Honor `requires` declarations so a selected skill automatically pulls in the same-source or same-catalog skills it depends on, transitively, while keeping approval and linkability guarantees intact. A selected skill whose required closure cannot be linked consistently must not materialize.

## Deliverables

- Walk the transitive `requires` closure within the same source/catalog (the `requires` field already exists on the inventory record) and add required same-source skills to the effective selection.
- Approval still applies: a required skill that would become active for the first time and is not covered by a skill-, source-, author-, or org-level approval blocks its dependent until approval is granted.
- A required ref is satisfied when an active linked skill fills its slot/ID — including the winner of that slot. A requirement that is already satisfied never blocks its dependent.
- Preflight linkability check: the dependent is blocked before materialization only when a required ref is missing, pending approval, shadowed-but-not-satisfied (shadowed by a non-equivalent winner), blocked by a same-name target entry, or otherwise unlinked.
- Cross-source `requires` are checked and reported only — never auto-installed across source boundaries.
- Optional best-effort warning when skill text mentions another skill by name; text inference must never be the sole blocker.
- Closure results surfaced in `status`/`doctor` (which selections expanded, which are blocked and why).

## Out of Scope

- Cross-source automatic installation.
- The full interactive resolve assistant.
- Catalog move reconciliation (M12).

## Acceptance Criteria

- A selected skill requiring another same-catalog skill pulls that skill into the effective selection.
- The expansion is transitive (a required skill's own requires are walked).
- A required ref that is missing, pending approval, shadowed-but-not-satisfied, same-name-blocked, or otherwise unlinked blocks the dependent during preflight; the dependent is not materialized.
- A requirement satisfied by an active linked skill (e.g. the winner of that slot) does not block the dependent.
- Cross-source requires are reported, not installed.
- Text-mention inference alone never blocks materialization.

## Validation

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --store /tmp/dalo-test --json status
git diff --check
```

## Suggested Issue Split

- Implement transitive same-source closure walking over `requires`.
- Enforce approval on first-time-active required skills.
- Add the required-closure linkability preflight and blockers.
- Report cross-source requires without installing them.
- Surface expanded/blocked closures in `status` and `doctor`.

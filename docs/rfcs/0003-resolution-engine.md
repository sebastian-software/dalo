# RFC 0003: Resolution Engine

Status: Draft  
Date: 2026-06-23  
Author: Sebastian + Claude  
Depends on: RFC 0001, RFC 0002  

## 1. Summary

This RFC specifies the three core engine behaviors that RFC 0001 and RFC 0002 describe only in prose:

1. the **resolver** — how active sources and their inventories become one deterministic resolved asset set,
2. the **store lock** — how concurrent invocations (interactive `sync`, autosync, agents writing into the store) are serialized safely,
3. **materialize reconciliation** — how the desired set, the recorded state, and the actual filesystem are reconciled into a safe operation plan.

The goal is to remove ambiguity before implementation and to give the test plan (RFC 0002 §11) concrete, enumerable scenarios. This RFC defines behavior and data flow, not Rust types or crate choices.

## 2. Dependencies and Assumptions

This RFC consumes decisions that are still open in their own issues. Where a decision is not yet final, this RFC states an **assumption** and marks it, so the resolver can be fully specified now and only confirmed later.

- **A1 — Conflict key is the skill `name`.** Shadowing and override are keyed on the non-source-qualified skill `name`, not on frontmatter `id`. Rationale: the materialization target is a directory named after the skill (for example `~/.claude/skills/copy-editing`), so the target filesystem physically allows only one entry per name. The conflict key is therefore forced by the target layout. Frontmatter `id` is used for catalog move-detection and dependency references, not for shadowing. *(Pending #2.)*
- **A2 — `name` source:** the display/conflict name is taken from `SKILL.md` frontmatter `name` when present, otherwise the skill folder name. A source-qualified reference has the form `<source-id>:<name>` (matching the user-lock example in RFC 0001 §11). *(Pending #2.)*
- **A3 — A managed skill never overwrites an unmanaged real directory.** A name collision between a resolved managed skill and an existing unmanaged directory is a `conflicted` outcome, reported and never auto-resolved. The guided resolution path is specified in #3; this RFC only needs the reconciliation to *detect and refuse* it. *(Pending #3.)*
- **A4 — Team-source freshness** (HEAD vs. pin) is out of scope here; the resolver consumes whatever commit a source is currently checked out at. *(Tracked in #1.)*

The resolver is a **pure function**: given inventories and config, it returns a resolved model plus diagnostics, with no filesystem or network effects. This is the property RFC 0002 §6/§11 relies on for testability.

## 3. Inputs and Outputs

### 3.1 Resolver inputs

- the active set of sources from config, each with `id`, `kind`, `priority`, `enabled`, `trusted`
- for each enabled source, an **inventory**: the scanned skills and instruction packs (RFC 0001 §5), already filtered for catalog sources to only the selected skills
- per-catalog dependency policy (`warn` | `include` | `block`, default `warn`; RFC 0001 §10)
- the previous resolved user lock (optional, used for drift comparison only)

Inventory production (scanning a checkout, scanning a catalog, computing fingerprints) happens **before** the resolver and is not part of it. The resolver never reads the filesystem.

### 3.2 Resolver output

A `Resolution` value containing:

- `active_skills`: one winner per conflict key, each with its source-qualified ref, store path, source, commit, and (if the winner is local) a `local_override` flag
- `shadowed_skills`: every non-winning skill, each with `shadowed_by`
- `active_instruction_packs`: ordered per target file (see §5)
- `diagnostics`: a flat list of typed findings (shadowing, dirty source, missing dependency, catalog drift, instruction topic overlap, …) mapping directly to the status states in RFC 0001 §16

`diagnostics` is the single channel the resolver uses to report everything non-fatal. The resolver does not print and does not decide exit codes; callers (`status`, `sync`, `doctor`) interpret the resolution.

## 4. Resolver Algorithm

Deterministic pipeline. Steps run in this order.

```text
1. select sources
   - keep enabled sources
   - sort by (priority asc, source_id asc)        # A: smaller priority wins
   - the (priority asc, source_id asc) order is the canonical tie-break

2. collect skill candidates
   - for each source in priority order:
       for each skill in source.inventory.skills:
         candidate = {
           conflict_key = skill.name,              # A1/A2
           source_ref   = "<source.id>:<skill.name>",
           source, path, commit, id (optional),
           requires (optional),
         }
   - catalog sources contribute only selected skills (already filtered)

3. group + shadow
   - group candidates by conflict_key
   - within each group, winner = first in canonical order
     (lowest priority number; ties broken by source_id)
   - winner.status = active
   - all others.status = shadowed, shadowed_by = winner.source_ref
   - if winner.source.kind == local and group.size > 1:
       winner.local_override = true
       emit diagnostic LOCAL_OVERRIDE(winner, shadowed_members)

4. dependency check (per active skill with `requires`)
   - for each required ref:
       resolve ref against active_skills (by id when ref is an id,
         else by name)                              # see §4.1
       if satisfied: ok
       else apply policy:
         warn  -> emit DEPENDENCY_MISSING (non-blocking)
         include -> attempt to also select the dependency if it exists
                    in the same catalog; else emit DEPENDENCY_MISSING
         block -> emit DEPENDENCY_MISSING(blocking = true)

5. resolve instruction packs                        # see §5

6. attach source-state diagnostics
   - dirty source            -> DIRTY (per affected skill/source)
   - orphaned ref            -> ORPHANED
   - catalog drift           -> NEW_AVAILABLE / SELECTED_CHANGED /
                                SELECTED_MOVED / SELECTED_REMOVED
   - (these come from inventory metadata, not recomputed here)

7. return Resolution { active_skills, shadowed_skills,
                       active_instruction_packs, diagnostics }
```

### 4.1 Dependency reference matching

`requires` entries may be a frontmatter `id` (for example `example.positioning`) or a source-qualified name (for example `oss:positioning`). Matching:

- if the entry contains `:` → match against an active skill's `source_ref`
- else if it looks like an `id` → match against active skills' frontmatter `id`
- else → match against active skills' `name`

Cross-source dependency satisfaction is allowed (an `oss` skill may satisfy a `company` skill's requirement) but is **only checked, never auto-installed** across sources. `include` policy auto-selection is limited to the **same catalog**, matching RFC 0001 §10. Whether `requires` should be restricted to ids only remains open in #2/#7.

### 4.2 Determinism guarantees

- Output ordering is stable: active and shadowed lists are sorted by `conflict_key`, then `source_ref`.
- No wall-clock, randomness, or map-iteration order influences the result.
- Re-running the resolver on identical inventories yields byte-identical serialized output (supports the snapshot tests in RFC 0002 §11).

## 5. Instruction Pack Resolution

Instruction packs resolve as a **separate asset type** (RFC 0001 §13). The full target-file mapping is decided in #4; this RFC specifies only the ordering and overlap rules the resolver owns:

- group active packs by the target instruction file they map to
- within a file, order by (source `priority` asc, pack `priority` asc, pack `id` asc)
- if two active packs declare the same `topic`, emit `INSTRUCTION_TOPIC_OVERLAP` (informational; no semantic inference)
- a local pack with the same `id` as a team pack overrides it; emit a visible override diagnostic, mirroring skill `local_override`

Per the V1-scope recommendation (#8), instruction packs may ship in V1.1 rather than V1. This section stands regardless of when it ships.

## 6. Store Lock

### 6.1 Why

Autosync, interactive commands, and agents editing symlinked store checkouts can run concurrently. RFC 0002 §8 specifies atomic single-file writes but no cross-operation serialization. A coarse store-level lock is sufficient and safe for V1.

### 6.2 Mechanism

- A single advisory exclusive lock at `~/.skillmgr/.lock` (distinct from `lock.toml`).
- Acquired via OS advisory file locking (`flock`-style). The lock file records the holder's `pid` and process start time for stale detection.
- Held for the whole duration of any **mutating** operation.

Operations are classified:

| Class | Commands | Lock |
| --- | --- | --- |
| mutating | `sync`, `adopt`, `promote`, `update`, `instruction enable/disable`, `target link/unlink`, `source add/remove/priority`, `init` | exclusive |
| read-only | `status`, `doctor`, `source list`, `instruction list`, `target detect` | none (or shared) |

Read-only commands take no lock so diagnostics always work, even while a sync is running. They may observe a transient state; that is acceptable and preferable to blocking inspection.

### 6.3 Contention behavior

- **Interactive mutating command** finds the lock held: retry briefly with backoff (a few seconds total), then fail with exit code `3` (unsafe state blocked, RFC 0002 §9) and a message naming the holder: `another skillmgr operation is running (pid <pid> since <time>)`.
- **`sync --auto`** (autosync) finds the lock held: do **not** wait and do **not** fail loudly. Log "skipped: store lock held by pid <pid>" and exit `0`. The next scheduled run retries. Autosync must never contend with an interactive session.

### 6.4 Stale lock recovery

- If the recorded `pid` is not alive (or the process start time does not match), the lock is stale. The acquiring process may take it over, logging the takeover.
- Lock acquisition and the staleness check are themselves done atomically (create-exclusive or `flock`), so two processes cannot both decide a lock is stale.

### 6.5 Scope

- One global store lock in V1. Finer-grained per-source locks are a future optimization and are out of scope.
- The lock guards store mutation only. It does not guard agent target directories; those are protected by the ownership rules in §7, not by the lock.

## 7. Materialize Reconciliation

### 7.1 Three states

For every **managed slot** — a skill `name` within a target directory, or an instruction block within a target file — reconciliation compares:

- **D (desired):** the slot is in the current `Resolution` and should exist
- **R (recorded):** `state.toml` records this slot as skillmgr-owned
- **A (actual):** the real filesystem state of the slot

Actual is one of: `absent`, `owned_correct` (symlink to the desired store path), `owned_wrong` (skillmgr-owned symlink to a different/stale path), `owned_broken` (skillmgr-owned dangling symlink), `foreign_symlink` (a symlink skillmgr does not own), `unmanaged_real` (a real file/directory), or `unmanaged_present` for blocks (unmarked content occupying the conceptual slot).

"Owned" means: recorded in `state.toml` **and** the symlink target points inside the canonicalized store (RFC 0002 §8). Both must hold; a symlink that merely looks like ours but is not recorded is treated as `foreign`.

### 7.2 Symlink reconciliation table

| D | R | A | Action |
| --- | --- | --- | --- |
| yes | yes | owned_correct | no-op |
| yes | yes | owned_wrong | relink to desired store path |
| yes | yes | owned_broken | recreate symlink |
| yes | yes | absent | recreate symlink (user removed it) |
| yes | yes | unmanaged_real | **conflict** — report, never touch (A3 / #3) |
| yes | no | absent | create symlink + record |
| yes | no | unmanaged_real | **conflict** — name collision, report (A3 / #3) |
| yes | no | foreign_symlink | **conflict** — report, do not touch |
| no | yes | owned_correct / owned_broken | remove symlink + drop record |
| no | yes | absent | drop record (already gone) |
| no | yes | unmanaged_real | drop record, do not touch |
| no | yes | foreign_symlink | drop record, report (left the ownership set) |
| no | no | anything | ignore (not skillmgr's concern) |

The **orphan** case (RFC 0001 §19.4) is `D=no` because a removed source/skill leaves the desired set; rows `no/yes/owned_*` cover it. Orphan removals are reported in `status`/`doctor` even though they are auto-applied.

### 7.3 Instruction block reconciliation

Analogous, with block markers instead of symlinks (RFC 0001 §13, §19.6):

- `owned_correct` = a `skillmgr:start/​end` block whose content matches the resolved pack
- `owned_wrong` = an owned block whose content differs → **drift**
- interactive `sync` may offer to restore drifted owned blocks; `sync --auto` reports and **blocks** on drift, never overwriting (RFC 0001 §19.6)
- malformed markers block writes to that file entirely (RFC 0002 §8)
- unmarked content is never a slot and is preserved byte-for-byte

### 7.4 Plan / apply split

Reconciliation runs in two phases (RFC 0002 §8):

1. **Plan:** from (D, R, A) per slot, produce an ordered list of typed operations: `Create`, `Relink`, `Remove`, `RestoreBlock`, `RemoveBlock`, `Conflict`, `NoOp`. `Conflict` operations carry the reason and are never executed.
2. **Apply:** execute non-conflict operations; then update `state.toml` to reflect what was actually achieved.

Rules:

- `--dry-run` prints the plan and applies nothing (RFC 0002 §8/§14).
- The plan is computed once and is what both the dry-run output and the real apply consume, so they cannot diverge.
- `state.toml` is updated only after the corresponding operation succeeds; a partial failure leaves a consistent record, and the next `sync` re-reconciles idempotently.
- A plan containing any blocking `Conflict` (or, under `--auto`, any drift/dirty block) stops the mutating apply for the affected slots, materializes the safe ones, and surfaces the rest. Whether a conflict is per-slot blocking or run-blocking follows RFC 0001 §23.

### 7.5 Idempotence

Running `sync` twice with no input change produces an all-`NoOp` plan on the second run. This is a required test (§8).

## 8. Test Scenarios

These extend RFC 0002 §11 with cases this RFC makes concrete:

Resolver (pure, no filesystem):

- equal names across sources resolve by priority; loser is `shadowed_by` winner
- equal priority numbers tie-break deterministically by `source_id`
- local winner over a team skill sets `local_override` and emits the diagnostic
- `requires` satisfied cross-source → ok; unsatisfied → policy-dependent warn/block
- identical inventories produce byte-identical serialized output (snapshot)

Store lock:

- second interactive mutating command fails with exit `3` while lock held
- `sync --auto` exits `0` and logs "skipped" while lock held
- a lock whose pid is dead is treated as stale and taken over

Reconciliation (temp store + temp target):

- user-deleted owned symlink is recreated
- owned symlink with stale target is relinked
- desired skill colliding with an unmanaged real directory yields `Conflict`, no filesystem change
- deselected skill removes only its owned symlink, leaving unmanaged files untouched
- orphaned owned symlink (source removed) is removed and reported
- drifted owned instruction block blocks under `--auto`, offers restore interactively
- malformed block markers block writes to that file
- second `sync` with no change is all `NoOp` (idempotence)
- every reconciliation test also runs under `--dry-run` and asserts zero filesystem mutation

## 9. Open Questions

- Confirm A1/A2 in #2 (conflict key, name source, `requires` reference forms).
- Confirm A3 resolution path in #3 (what the guided fix for a managed↔unmanaged collision is; this RFC only refuses the collision).
- Whether read-only commands should take a shared lock or no lock at all.
- Whether the store lock should also cover the local source's Git operations or leave those to Git's own index lock.
- Backoff/timeout numbers for interactive lock contention (left as implementation detail for now).
- How drift restoration interacts with converting a local block edit into a local instruction pack (RFC 0001 §19.6); detailed `resolve` behavior is still open in RFC 0001 §25.

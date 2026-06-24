# RFC 0003: Resolution Engine

Status: Draft  
Date: 2026-06-23  
Author: Sebastian + Claude  
Depends on: RFC 0001, RFC 0002  

## 1. Summary

This RFC specifies the three core engine behaviors that RFC 0001 and RFC 0002 describe only in prose:

1. the **resolver** — how active sources and their inventories become one deterministic resolved asset set,
2. the **store lock** — how concurrent invocations (interactive sync, scheduled sync, agents writing into the store) are handled safely,
3. **materialize reconciliation** — how the desired set, the recorded state, and the actual filesystem are reconciled into a safe operation plan.

The goal is to remove ambiguity before implementation and to give the test plan (RFC 0002 §11) concrete, enumerable scenarios. This RFC defines behavior and data flow, not Rust types or crate choices.

## 2. Dependencies and Assumptions

This RFC consumes decisions from RFC 0001 and records remaining assumptions only where later issues still own the detailed UX.

- **D1 — Conflict key is the skill slot name.** Shadowing and override are keyed on the non-source-qualified slot name, not on frontmatter `id`. Rationale: the materialization target is a directory named after the skill (for example `~/.claude/skills/copy-editing`), so the target filesystem physically allows only one entry per slot name. Frontmatter `id` is stable identity metadata for catalog move-detection and dependency references, not the physical install key.
- **D2 — Slot name source:** the slot name is taken from `SKILL.md` frontmatter `name` when present and valid, otherwise the skill folder name. A source-qualified reference has the form `<source-id>:<slot-name>` (matching the user-lock example in RFC 0001 §11).
- **D3 — A managed skill never overwrites an unmanaged target entry.** A slot-name collision between a resolved managed skill and an existing unmanaged real directory or foreign symlink is a `conflicted` outcome. V1 reports the conflict, never overwrites the unmanaged entry, and materializes other safe slots where possible. Guided resolution is explicit: adopt into local source, keep/protect unmanaged, explicitly replace after preservation and confirmation, or use a later rename/adapt flow.
- **D4 — Team-source freshness is outside the resolver.** `sync` refreshes clean tracking team sources before inventory production; `source refresh` advances pinned source locks. The resolver consumes whatever commit each source checkout is currently at.

The resolver is a **pure function**: given inventories and config, it returns a resolved model plus diagnostics, with no filesystem or network effects. This is the property RFC 0002 §6/§11 relies on for testability.

## 3. Inputs and Outputs

### 3.1 Resolver inputs

- the active set of sources from config, each with `id`, `kind`, `priority`, `enabled`, `trusted`
- for each enabled source, an **inventory**: the scanned skills and instruction packs (RFC 0001 §5). For a catalog source the inventory is the **full** discovered set, paired with the source's explicit selection; the resolver applies selection and required-closure expansion itself (§4 step 2), so it must receive the unselected catalog skills too
- local approval state for skill-, source-, author-, and org-level approvals (RFC 0001 §20)
- the previous resolved user lock (optional, used for drift comparison only)

Inventory production (scanning a checkout, scanning a catalog, computing fingerprints) happens **before** the resolver and is not part of it. The resolver never reads the filesystem.

### 3.2 Resolver output

A `Resolution` value containing:

- `active_skills`: one winner per slot-name conflict key, each with its source-qualified ref, store path, source, commit, and (if the winner is local) a `local_override` flag
- `pending_approval_skills`: would-be winners that are not materialized because no approval rule covers them yet
- `shadowed_skills`: every non-winning managed skill, each with `shadowed_by`; callers should present these as unlinked skills with reason `shadowed`
- `active_instruction_packs`: ordered per target file (see §5)
- `diagnostics`: a flat list of typed findings (shadowing, blocked same-name target entries, dirty source, missing dependency, catalog drift, instruction topic overlap, …) mapping directly to the status states in RFC 0001 §16

`diagnostics` is the single channel the resolver uses to report everything non-fatal. The resolver does not print and does not decide exit codes; callers (`status`, `sync`, `doctor`) interpret the resolution.

## 4. Resolver Algorithm

Deterministic pipeline. Steps run in this order.

```text
1. select sources
   - keep enabled sources
   - sort by (priority asc, source_id asc)        # smaller priority wins
   - the (priority asc, source_id asc) order is the canonical tie-break

2. resolve selections (+ required closure expansion)
   - for each catalog source, start from its explicit selected set
   - for each selected skill, repeatedly add any same-source or same-catalog
     skill that is declared in `requires`, looked up in the FULL source/catalog
     inventory, until no new skill is added
   - the result is the source/catalog's effective selected set
   - this step is the reason the resolver receives the full catalog
     inventory rather than a pre-filtered one (§3.1)

3. collect skill candidates
   - for each source in priority order:
       for each skill in source (catalog sources: effective selected set):
         candidate = {
           conflict_key = skill.slot_name,         # D1/D2
           source_ref   = "<source.id>:<skill.slot_name>",
           source, path, commit, id (optional),
           requires (optional),
         }

4. group + approve + shadow
   - group candidates by conflict_key
   - within each group, winner = first approved candidate in canonical order
     (lowest priority number; ties broken by source_id)
   - winner.status = active
   - unapproved candidates that would otherwise win become pending_approval
     and are not materialized
   - all non-winning approved candidates become shadowed, shadowed_by = winner.source_ref
   - if no approved candidate exists, the group has no active skill until approval
   - if winner.source.kind == local and group.size > 1:
       winner.local_override = true
       emit diagnostic LOCAL_OVERRIDE(winner, shadowed_members)

5. dependency preflight (per approved active skill with `requires`)
   - for each required ref:
       resolve ref against active_skills (by id when ref is an id,
         else by source-qualified ref or slot name) # see §4.1
       if satisfied by an active linked skill: ok
       else emit DEPENDENCY_MISSING or DEPENDENCY_BLOCKED (blocking)
   - if a selected skill's required closure contains any missing, pending
     approval, shadowed-but-not-satisfied, same-name-blocked, or otherwise
     unlinked required skill, block the dependent selected skill before
     reconciliation

6. resolve instruction packs                        # see §5

7. attach source-state diagnostics
   - dirty source            -> DIRTY (per affected skill/source)
   - orphaned ref            -> ORPHANED
   - pending approval        -> PENDING_APPROVAL
   - catalog drift           -> NEW_AVAILABLE / SELECTED_CHANGED /
                                SELECTED_MOVED / SELECTED_REMOVED
   - (these come from inventory metadata, not recomputed here)

8. return Resolution { active_skills, pending_approval_skills,
                       shadowed_skills, active_instruction_packs,
                       diagnostics }
```

### 4.1 Dependency reference matching

`requires` entries support three first-class reference forms:

- stable frontmatter IDs, for example `example.positioning`
- source-qualified refs, for example `oss:positioning`
- same-source relative slot names, for example `positioning`

Same-source relative slot names are a permanent compatibility requirement for external multi-skill repositories whose skills refer to each other by local names. They are not treated as deprecated or best-effort. Matching:

- if the entry contains `:` → match against an active skill's `source_ref`
- else if it looks like an `id` → match against active skills' frontmatter `id`
- else → match against a skill's slot name within the declaring source/catalog only

Cross-source dependency satisfaction is allowed only for stable IDs or source-qualified refs and is **checked, never auto-installed** across sources. Same-source and same-catalog dependency expansion happens in §4 step 2. Visible aliases are never dependency targets.

### 4.2 Determinism guarantees

- Output ordering is stable: active and shadowed lists are sorted by `conflict_key`, then `source_ref`.
- No wall-clock, randomness, or map-iteration order influences the result.
- Re-running the resolver on identical inventories yields byte-identical serialized output (supports the snapshot tests in RFC 0002 §11).

## 5. Instruction Pack Resolution

Instruction packs resolve as a **separate asset type** (RFC 0001 §13). V1.1 materializes them through explicitly configured target instruction files and skillmgr-owned managed blocks, not through assumed native include/import support. Mapping configuration belongs to RFC 0001; this resolver RFC owns only ordering and overlap rules:

- group active packs by the target instruction file they map to
- within a file, order by (source `priority` asc, pack `priority` asc, pack `id` asc)
- if two active packs declare the same `topic`, emit `INSTRUCTION_TOPIC_OVERLAP` (informational; no semantic inference)
- a local pack with the same `id` as a team pack overrides it; emit a visible override diagnostic, mirroring skill `local_override`

Per RFC 0001 §26, instruction packs ship in V1.1 rather than V1. This section stands regardless of when it ships.

## 6. Store Lock

### 6.1 Why

Interactive commands and scheduled sync both mutate the store and can run concurrently. RFC 0002 §8 specifies atomic single-file writes but no cross-operation serialization. A coarse store-level lock closes that gap.

The lock serializes **skillmgr operations against each other** only. It does **not** capture writes made by external agents that edit a materialized skill through its symlink into a source checkout — those processes never invoke skillmgr and so never take the lock. Such edits are handled as **dirty state**, not by the lock: every mutating operation first runs a dirty check (`git status --porcelain=v2`, RFC 0002 §5) on each source checkout it would touch, and scheduled or non-interactive `sync` blocks on a dirty source instead of overwriting it (RFC 0001 §19.3). The two are complementary — the lock stops two skillmgr runs from racing, the dirty check stops skillmgr from clobbering an agent's in-flight edit.

### 6.2 Mechanism

- A single advisory exclusive lock at `~/.skillmgr/.lock` (distinct from `lock.toml`).
- Acquired via OS advisory file locking (`flock`-style). The lock file records the holder's `pid` and process start time for stale detection.
- Held for the whole duration of any **mutating** operation.

Operations are classified:

| Class | Commands | Lock |
| --- | --- | --- |
| mutating | `sync`, `adopt`, `promote`, `source refresh`, `instruction enable/disable`, `target link/unlink`, `source add/remove/priority`, `init` | exclusive |
| read-only | `status`, `doctor`, `source list`, `instruction list`, `target detect` | none |

Read-only commands take no lock so diagnostics always work, even while a sync is running. They may observe a transient state; that is acceptable and preferable to blocking inspection.

### 6.3 Contention behavior

- **Interactive mutating command** finds the lock held: retry immediately, then after 100 ms, 250 ms, 500 ms, 1 s, and 2 s. If the lock is still held after about 5 seconds total, fail with exit code `3` (unsafe state blocked, RFC 0002 §9) and a message naming the holder: `another skillmgr operation is running (pid <pid> since <time>)`.
- **Scheduled sync** finds the lock held: do **not** wait and do **not** fail loudly. Log "skipped: store lock held by pid <pid>" and exit `0`. The next scheduled run retries. Scheduled sync must never contend with an interactive session.

### 6.4 Stale lock recovery

- If the recorded `pid` is not alive (or the process start time does not match), the lock is stale. The acquiring process may take it over, logging the takeover.
- Lock acquisition and the staleness check are themselves done atomically (create-exclusive or `flock`), so two processes cannot both decide a lock is stale.

### 6.5 Scope

- One global store lock in V1. Finer-grained per-source locks are a future optimization and are out of scope.
- The lock guards store mutation only. It does not guard agent target directories; those are protected by the ownership rules in §7, not by the lock.
- Local source Git operations are not wrapped in a separate skillmgr Git lock in V1. Git's own index lock remains the authority for concurrent Git operations.

## 7. Materialize Reconciliation

### 7.1 Three states

For every **managed slot** — a skill slot name within a target directory, or an instruction block within a target file — reconciliation compares:

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
| yes | yes | unmanaged_real | **conflict** — report, never touch (D3) |
| yes | yes | foreign_symlink | **conflict** — recorded slot was replaced by a non-owned symlink; report, never relink over it |
| yes | no | absent | create symlink + record |
| yes | no | unmanaged_real | **conflict** — slot-name collision, report, never touch (D3) |
| yes | no | foreign_symlink | **conflict** — report, do not touch |
| no | yes | owned_correct / owned_broken | remove symlink + drop record |
| no | yes | absent | drop record (already gone) |
| no | yes | unmanaged_real | drop record, do not touch |
| no | yes | foreign_symlink | drop record, report (left the ownership set) |
| no | no | anything | ignore (not skillmgr's concern) |

The **orphan** case (RFC 0001 §19.5) is `D=no` because a removed source/skill leaves the desired set; rows `no/yes/owned_*` cover it. Orphan removals are reported in `status`/`doctor` even though they are auto-applied.

### 7.3 Instruction block reconciliation

Analogous, with block markers instead of symlinks (RFC 0001 §13, §19.7):

- `owned_correct` = a `skillmgr:start/​end` block whose content matches the resolved pack
- `owned_wrong` = an owned block whose content differs → **drift**
- interactive `sync` may offer to restore drifted owned blocks; scheduled or non-interactive `sync` reports and **blocks** on drift, never overwriting (RFC 0001 §19.7)
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
- A plan containing any blocking `Conflict` (or, in scheduled/non-interactive sync, any drift/dirty block) stops the mutating apply for the affected slots, materializes the safe ones, and surfaces the rest. Whether a conflict is per-slot blocking or run-blocking follows RFC 0001 §23.

### 7.5 Idempotence

Running `sync` twice with no input change produces an all-`NoOp` plan on the second run. This is a required test (§8).

## 8. Test Scenarios

These extend RFC 0002 §11 with cases this RFC makes concrete:

Resolver (pure, no filesystem):

- equal slot names across sources resolve by priority; loser is `shadowed_by` winner
- equal priority numbers tie-break deterministically by `source_id`
- local winner over a team skill sets `local_override` and emits the diagnostic
- unapproved would-be winner moves to `pending_approval_skills` and is not active
- source-, author-, or org-level approval allows matching newly active skills without per-skill approval
- `requires` satisfied cross-source → ok; unsatisfied → blocking diagnostic for the dependent selected skill
- same-source relative `requires` resolves inside the declaring source/catalog and does not search other sources
- required closure expansion adds same-source and same-catalog dependencies into the effective selected set
- a required skill that is missing, pending approval, shadowed by a non-equivalent winner, or blocked by a target collision prevents the dependent selected skill from materializing
- identical inventories produce byte-identical serialized output (snapshot)

Store lock:

- second interactive mutating command fails with exit `3` while lock held
- scheduled sync exits `0` and logs "skipped" while lock held
- a lock whose pid is dead is treated as stale and taken over

Reconciliation (temp store + temp target):

- user-deleted owned symlink is recreated
- owned symlink with stale target is relinked
- created skill links are directory-level symlinks to resolved store paths
- edits through a materialized team-skill symlink make the underlying source checkout dirty
- desired skill colliding with an unmanaged real directory yields `Conflict`, no filesystem change
- a recorded slot replaced by a foreign (outside-store) symlink yields `Conflict`, never a relink
- deselected skill removes only its owned symlink, leaving unmanaged files untouched
- orphaned owned symlink (source removed) is removed and reported
- drifted owned instruction block blocks under scheduled/non-interactive sync, offers restore interactively
- malformed block markers block writes to that file
- second `sync` with no change is all `NoOp` (idempotence)
- every reconciliation test also runs under `--dry-run` and asserts zero filesystem mutation

## 9. Open Questions

There are no blocking open resolver questions for V1.

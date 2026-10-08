# ADR 0010: Project Declarations as Approval Authority

Status: Accepted  
Date: 2026-10-08  
Related: [Issue 939](https://github.com/sebastian-software/dalo/issues/939)  
Amends: [ADR 0009](0009-explicit-project-scope.md)

## Context

ADR 0009 registers every project source as an untrusted catalog and requires
local, content-bound approvals in each project store. Every clone and every
worktree has its own store, so each one starts with pending approvals, and a
fresh `dalo install` exits nonzero. A `postinstall` hook, an agent, or CI cannot
restore the reviewed project without someone approving each skill again.

That gate does not protect against the repository. A repository that wants to
deliver a skill can commit the same content into `.claude/skills/` or run code
through `postinstall`. A full commit pin in a reviewed declaration is equivalent
to committing the pinned content. Repository trust is therefore the actual
boundary in project scope. "The developer deliberately installed it" is not a
boundary either: `postinstall`, agents, and CI are not deliberate acts.

Per-developer approvals in project scope also make the effective agent setup of
the same commit depend on each machine's local state, which defeats the purpose
of a shared, reproducible declaration.

## Decision

`dalo-project.toml` schema version 2 adds an optional top-level `approval`
field:

- `"local"`, the default, keeps the ADR 0009 behavior unchanged.
- `"declaration"` makes the reviewed declaration the approval authority for its
  project store. Every explicitly selected skill and its resolved required
  closure is approved without local approval records.

Schema version 1 keeps its exact semantics, and `approval` is rejected in schema
version 1. Unknown values fail closed. Dalo versions that predate schema version
2 reject it, so a team opts in only once every member uses a supporting
version. `init` keeps writing schema version 1, and the declaration editors
preserve the schema version, the field, and its formatting.

`install` reconciles each declared source's persisted `trusted` flag from the
declared mode on every run, through the existing journaled snapshot path:
`declaration` registers trusted catalogs, `local` registers untrusted catalogs.
A trusted catalog still activates only its explicit selection and its required
closure. Until `install` has reconciled the store, `status`, `doctor`, `audit`,
`approve`, `project add`, and `project update` refuse a store whose flags
differ from the declaration.

This amends two statements of ADR 0009 for opted-in projects: "Declarations
cannot grant trust" and the reuse of "local content-bound approvals". The rest
of ADR 0009 is unchanged.

The following stay as they are:

- Deterministic audits run on every sync, and unaccepted high or critical
  findings block delivery. Accepted risks remain local and bound to the exact
  audited content (`dalo audit <source:skill> --accept-risk "<reason>"`).
- Dirty sources, pin checks, unmanaged same-name entries, redirected paths, and
  the project ownership receipt fail closed exactly as before.
- `install` never creates approval records. Existing local records are preserved
  but not needed while the mode is `declaration`, and they apply again after a
  switch back to `local`. Changed or removed content still revokes them.
- There are no local opt-outs. A deviation belongs in the declaration and goes
  through the repository's review.

The declaration approves everything a project delivers project-locally. Today
that is skills only, because project scope does not support plugins, tools,
hooks, or agents. A project installation never has global effects; anything
that does is a defect, not an approval question.

## Consequences

- A fresh clone, a new worktree, a `postinstall` hook, and CI restore the same
  reviewed skill set with one `dalo install`, without per-machine approvals.
- Review moves to the repository's pull request process. Changing a pin,
  selection, or the approval mode is a declaration change that reviewers must
  read like a dependency change. The first install after opting in, or after an
  update, lists the newly activated skills as ordinary sync operations.
- Human `install` and `status` output names the active approval mode, and the
  `--dry-run --json install` preview reports it in an additive `approval` field.
- Teams that keep `approval = "local"` or schema version 1 see no change.

What Dalo does not protect against in this mode:

- An untrusted or compromised repository. Installing an opted-in project grants
  the declaration's selection the same trust as content committed to that
  repository. Do not install a repository you would not otherwise build or run.
- A malicious or careless declaration change that passes review. The audit gate
  still blocks known high-risk patterns, but it does not make reviewed content
  safe.

Follow-up work that is not part of this decision: a CI-capable command that
fetches, audits, and shows the content and closure of declared pins for pull
request review; declaration-level risk acceptances; and team `dalo.toml`
catalogs as approval authority in global scope.

## Evidence

CLI tests use real local Git repositories and isolated homes to verify fresh
installs, a second clone, and a `git worktree` without approval records;
declaration updates to changed content; blocking audits with a local risk
acceptance; switching back to pending with preserved local approvals; the
selection and closure boundary of a trusted catalog; dirty sources and unmanaged
targets; editor preservation of schema version 2; and fail-closed parsing.

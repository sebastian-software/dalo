# Dalo Security Overview

This page is for the person who has to decide whether Dalo may deliver
third-party skills to every developer machine on a team. It describes what Dalo
trusts, what it checks, what it cannot check, and what your team still has to do
itself.

It is not the vulnerability reporting policy. To report a suspected
vulnerability privately, see [SECURITY.md](../SECURITY.md).

Every statement here maps to behavior in the CLI. Where a guarantee has a limit,
the limit is written down rather than left out.

## The one-paragraph summary

Dalo treats skill content as untrusted data. Adding a source is your trust
decision; everything after that is mechanical. A deterministic, local preflight
scans every skill before it can reach an agent folder, and high or critical
findings block the sync until a human records an explicit, content-bound
exception. Executables that ship with a skill set — tools and hooks — are
approved separately from the skills and from each other, by exact contract hash.
None of this makes a skill safe. It makes the decision to run a skill explicit,
reviewable, and reproducible.

## Trust boundaries

**Adding a source is the trust decision.** Dalo has two ways in, and they differ
in trust, not in mechanics:

| Command | Meaning | Update policy |
| --- | --- | --- |
| [`dalo source add`](reference.md#dalo-source-add-id-git-url-or-path---namespace-prefix) | A team source you vouch for | `track` |
| [`dalo source add-catalog`](reference.md#dalo-source-add-catalog-id-git-url-or-path---namespace-prefix) | An untrusted catalog of offers | `pin` |

Both clone into the store and run the deterministic preflight for every
discovered skill. Neither materializes anything on its own.

The difference is what happens next. A source added with `dalo source add` is
recorded as trusted, and the resolver treats every skill from a trusted source
as approved — that is the whole point of the trust decision, and it is why the
decision deserves the same scrutiny as adding a dependency to a build. A catalog
is the opposite: it starts untrusted with an empty selection.

**Catalog skills stay pending until approved.** A catalog skill is an offer. It
becomes active only after an explicit selection (`dalo source select`) and an
explicit approval ([`dalo approve skill`](reference.md#dalo-approve)). Until
then `dalo status` lists it under pending approvals, the user lock records it as
`pending_approval`, and `dalo status --check` exits non-zero.

**Trust removes approval, not the security gate.** Local skills and skills from
a source marked `trusted = true` need no approval record, as documented in
[`approvals.toml`](reference.md#approvalstoml). They are still audited on every
sync, and a blocking finding still stops materialization.

**Approvals are source-qualified.** An approval value is
`<source-id>:<skill>`, never a bare name, so an approval granted for one source
cannot be matched by a same-named skill in another source. The wider `source`,
`author`, and `org` scopes are also source-qualified, and a legacy bare approval
from an older Dalo is reported for re-approval rather than honored.

**Approvals live in your own store.** `approvals.toml` sits at the root of the
personal store — `~/.dalo` by default, relocatable with `--store` or
`DALO_STORE`. There is no repository-level approval file: a team `dalo.toml`
declares sources and pins, never approvals.

**Scheduled and non-interactive runs never grant approvals.** Autosync installs
a recurring [`dalo sync --check`](reference.md#dalo-autosync-installstatusuninstall)
through launchd, a systemd user timer, or cron. Pending approvals, security
findings, dirty sources, and target conflicts stay fail-closed: the run records
`blocked` with a reason instead of proceeding. Non-interactive commands can use
approvals that already exist but never create new ones.

**Incoming team updates are staged before they are trusted.** A tracking source
fetches into a detached worktree below `sources/.audit-staging/` and is audited
there. The live checkout fast-forwards only after the audit passes, so target
links keep exposing the last accepted commit while an update is blocked.

## The deterministic preflight

The preflight is local, reads files, and never executes skill code. It runs on
`source add`, on catalog selection and approval, on
[`dalo audit`](reference.md#dalo-audit-skill-or-path), and — against the exact
content hash of every active skill — before
[`dalo sync`](reference.md#dalo-sync) changes any link.

It walks every entry in the skill directory, then scans each text file line by
line. These are the rules it can report:

| Finding | Severity | What it means |
| --- | --- | --- |
| `static.destructive-root-command` | critical | A command can recursively delete a root or home directory |
| `static.remote-code-execution` | high | Remote content is downloaded and handed straight to an interpreter |
| `static.encoded-execution` | high | Content is decoded or evaluated as executable instructions |
| `static.persistence` | high | A startup file, scheduled task, or agent configuration may be modified |
| `static.privileged-execution` | high | Privileged command execution is requested |
| `static.sensitive-data-network-combination` | high | Sensitive-data access and outbound network behavior appear in the same skill |
| `static.symlink` | high | A symlink is opaque and may escape the skill directory |
| `static.git-metadata-entry` | high | The skill carries a `.git` entry, which is never materialized or reviewed |
| `static.special-filesystem-entry` | high | An entry is neither a regular file nor a directory |
| `static.instruction-override` | medium | Language commonly used to override higher-priority instructions |
| `static.sensitive-data-access` | medium | A credential or sensitive user-data location is referenced |
| `static.dynamic-execution` | medium | Dynamically constructed command execution |
| `static.oversized-file` | medium | A file above 1 MiB is too large for the content scan |
| `static.opaque-file` | medium | A non-text file could not be inspected |
| `static.executable-file` | low | The skill contains a file with an executable bit |

**High and critical findings block.** The report's status is `blocked` when any
finding is `high` or higher and no risk acceptance covers it. `medium` and below
produce a `review` status: they appear in the report and they do not stop a sync
on their own. `dalo audit --check` exits non-zero only for unaccepted `high` or
`critical` findings. That line is deliberate — persistence
and privilege escalation are high-confidence primitives, while dynamic execution
appears in many legitimate technical skills.

**Unscannable content is a finding, not a gap.** Oversized, non-text, symlinked,
special, and `.git` entries mark the report's coverage as `partial` and are
reported. Dalo does not silently pass over what it could not read.

**Exceptions are explicit and content-bound.** `--accept-risk "<reason>"` records
a reason together with a hash over the source provenance, engine versions,
coverage, and the exact finding set. A changed skill, a changed source, or a
newly discovered finding invalidates the acceptance and requires a fresh
decision.

**Reports are cached, not authoritative forever.** Reports live under
`audits/` in the store, keyed by the source-qualified reference and the full
directory hash, and Dalo restricts the directory and files to the current user.
Missing, malformed, old-version, or changed-content reports are rebuilt before
they can influence a result. This is local trust state, not a signed log: a
process running as the same user can replace it, so do not place the store in a
shared or untrusted writable directory.

## The optional agent reviewer

`dalo audit --reviewer auto|codex|claude|opencode` and
`dalo approve skill --reviewer ...` add a semantic layer on top of the
deterministic one. `sync` never starts a reviewer on its own.

What Dalo does to constrain it:

- A fresh, non-persistent provider process per review, with user configuration,
  project rules, skills, plugins, and MCP servers disabled where the provider
  supports it.
- A bounded snapshot of the skill (at most 512 KiB, `.git` always excluded),
  wrapped in `<untrusted_skill_snapshot>` markers and introduced by a prompt
  that says the content is data and never instructions.
- Claude and OpenCode run with tools denied. Codex keeps its read-only,
  network-disabled sandbox shell, so Dalo never picks Codex through `auto`;
  `--reviewer codex` is an explicit acceptance of the weaker boundary.
- The provider process receives an explicit environment allowlist — runtime
  variables and provider authentication only — instead of inheriting Dalo's
  environment.
- The reviewer's output must match a fixed JSON schema, and every finding must
  cite a snapshot-relative path.

**A passed review is not a safety guarantee.** The reviewer is a model reading
attacker-controlled text, and it is the layer an attacker can most plausibly
talk to. A skill can try to argue its way to an empty finding list, and the
free-text `summary` and `expected_actions` fields are model-controlled. So Dalo
treats the review as additive only:

- It can raise severity and add findings. The overall status is the maximum
  across layers.
- It can never clear a deterministic finding, approve, endorse, or certify a
  skill.
- Zero findings means *this constrained assessment found nothing more* — it is
  not an endorsement.

The block decision therefore stays anchored to the deterministic layer, which
the reviewed skill cannot influence.

One operational note: depending on the installed provider, a review sends skill
contents to an external model provider and consumes that provider's quota.

## Tools and hooks

Skills are text. Portable plugins can also ship *tools* (executables) and
*hooks* (contracts that run a tool on an agent event). These are approved
separately from skills and separately from each other.

**Approval is content-addressed.** The approval records are exact:

```text
tool    <source-id>:<plugin>#tool:<name>@sha256:<contract-hash>
hook    <source-id>:<plugin>#hook:<name>@sha256:<contract-hash>
```

The hash is a SHA-256 over the whole contract, not over the executable alone: a
tool hash covers its entry, runtime, argument vector, declared inputs,
environment allowlist, capabilities, platforms, and every staged file. A hook
hash covers its event semantics, matcher, bindings, timeout, failure policy —
and the hash of the tool it calls.

A tool approval validates the declared executable closure and stages it
immutably under `tools/`, addressed by its exact staged content, without running
it. A hook approval grants one exact hook contract and is refused until the tool
it references is independently ready.

**What re-triggers approval.** Any change to a hashed field produces a different
identity that nobody has approved. A new upstream version, an edited `argv`, a
changed matcher or binding, a different staged file — each one lands as
`pending approval`, or as `hash drift` when an earlier hash for that identity
was approved. Because a hook's hash includes its tool's hash, approving a new
tool version does not silently re-arm the hook that calls it. There is no
wildcard and no "approve this plugin forever" scope for tools and hooks.

**Approval is rechecked at execution time.** A hook is installed into the
provider as a stored projection, and a stored projection outlives its approvals.
Before it runs a handler, Dalo re-reads `approvals.toml` and requires a current
exact approval for both the hook and its tool, re-verifies the staged closure,
and recomputes the contract hash. A revoked or drifted hook aborts the dispatch
with a non-zero exit instead of silently skipping, so revocation disables an
already-installed sidecar.

**Inspection never executes.** `dalo tool list|show|audit` and
`dalo hook list|show` read contracts, recompute hashes, and report state; they
do not stage, approve, project, or run anything. See
[Tools](reference.md#tools) and [Hooks](reference.md#hooks) for the contract
fields, and [`dalo approve`](reference.md#dalo-approve) for the grant and revoke
commands.

**Contracts are closed.** Unknown fields are rejected, `timeout_ms` is bounded
to 100–120000 ms, `fail_closed` is only valid for pre-action enforcement
effects, and a subject/phase/effect combination must have a verified mapping in
a supported provider. Malformed, unknown, unsafe, or unsupported contracts are
blocked rather than passed through. Handler output is bounded and validated
against the declared effect before it is used.

**Know what a hook approval grants.** A hook is not an observer by default.
Depending on its declared effect it can deny a tool call, rewrite a tool's
input, replace the model-facing result, inject context into the session, or
force another agent turn. Approving a hook is a decision to let that plugin
influence what your agent does — read the effect, not only the name.

## The OS sandbox

Dalo normally links skill directories and runs nothing from a source. There is
exactly one case where it executes source-provided code during `sync`: a
[generated delivery](reference.md#deliverytoml-provider-builds), where an
approved tool builds the provider-specific artifact. That generator runs inside
an operating-system sandbox that every descendant process inherits.

| Platform | Mechanism | Filesystem | Network |
| --- | --- | --- | --- |
| Linux | Landlock, ABI v4, fully enforced | Writes only below the one delivery staging directory | TCP connect and bind denied |
| macOS | Seatbelt (`sandbox-exec`) with a generated profile | Writes only below the one delivery staging directory | All network denied |
| Anything else | None available | — | Generator execution is refused |

The sandbox is fail-closed. Landlock is requested as a hard requirement, so an
older kernel makes the generator refuse to run rather than fall back to a weaker
boundary. If the domain is not fully enforced, Dalo does not start the process.
On a platform without either mechanism, generated delivery is simply not
available.

Beyond the sandbox, the generator is started with a cleared environment, an
empty `PATH`, no stdin, its own process group, a bounded runtime, and bounded
output; it must be the exact approved staged executable.

Its limits, stated plainly:

- **It is not a read boundary.** The generator can read whatever its OS user can
  read. Do not treat a generated delivery as proof that its inputs stayed
  private.
- **Linux network control is TCP-only.** Landlock does not cover UDP or
  UNIX-domain sockets.
- **It covers generated delivery, not hooks.** A hook handler runs with a
  cleared environment, its own process group, and a bounded timeout, but not
  inside Landlock or Seatbelt. Approving a hook is permission to run that exact
  executable with your user's authority.
- **It does not constrain your agent.** The sandbox confines a process Dalo
  starts, not what your agent later does with text a skill supplied.

## Credentials

Dalo stores no secrets. There is no credential store, no token file, and no
configuration field for a password or key anywhere in the store.

Git authentication stays with Git. Dalo shells out to the `git` you already have
and lets your existing SSH agent or credential helper do the work; it never
reads those credentials itself. Interactive prompting is disabled
(`GIT_TERMINAL_PROMPT=0`, `ssh -oBatchMode=yes`, and a closed stdin), so a
hanging password prompt becomes a clear failure instead of a stuck process.

**A credential in a URL is rejected, not stored.** `https://user:token@host/…`
fails validation when the source is added, so a token never reaches
`config.toml`. The error points you at a local path, an SSH URL, or a credential
helper instead.

**Userinfo that does appear is redacted.** Where a URL still carries userinfo —
in Git's own stderr, in a failed command's recorded arguments, in progress
output, or in recorded source provenance — Dalo rewrites it to `***@host` before
printing or persisting it, so a pasted error report does not leak a token.

## Input hardening

Skill repositories are untrusted input, and parsing them is an attack surface of
its own:

- **Bounded frontmatter.** `SKILL.md` frontmatter is capped at 64 KiB, at 64
  levels of flow nesting, and at 16 YAML anchor or alias references — the last
  one because alias expansion is otherwise unbounded work. Metadata past a cap
  is not parsed: the skill is skipped with a `malformed frontmatter` inventory
  warning rather than activated with missing fields.
- **Bounded names.** A slot name is at most 120 characters of `[a-z0-9._-]`,
  with `.`, `..`, leading dots, and Windows-reserved basenames rejected. Source
  IDs must be a single path component of letters, digits, `.`, `_`, and `-`,
  and `.` and `..` are rejected, so neither can escape the store layout.
- **Transport allow-list.** A source URL must use `https`, `ssh`, `git`, or
  `file`, a local path, or a constrained `user@host:path` form. Schemes such as
  `http://`, `git+ssh://`, and especially `ext::<command>` — which would hand a
  command to Git as a transport helper — are rejected before Git is spawned.
  The same allow-list is passed to Git itself through `GIT_ALLOW_PROTOCOL`, and
  `GIT_PROTOCOL_FROM_USER=0` keeps a command-line URL from counting as
  user-approved. Clone arguments are separated with `--`, so a URL starting
  with `-` cannot become an option. Submodules are never requested, so no
  submodule content is fetched or executed.
- **Bounded scanning.** Files above 1 MiB are not content-scanned and the
  reviewer snapshot is capped at 512 KiB — both produce a visible finding or
  marker rather than silence. Evidence snippets in a report are truncated, so
  one crafted line cannot flood the output.
- **Closed schemas.** User-authored and persisted TOML rejects unknown fields
  and unsupported schema versions instead of guessing.
- **Escaped terminal output.** Repository-controlled text — names, descriptions,
  paths, evidence snippets, Git's own stderr — has its control characters
  escaped before it reaches a human terminal, so a crafted file cannot inject
  ANSI sequences into Dalo's output or hide text from you. Color codes are
  emitted only for a fixed set of Dalo's own status words. `--json` output is
  left
  unescaped on purpose: JSON encoding handles it, and consumers are programs.

## What Dalo does not protect against

This list matters more than the previous ones. Dalo raises the cost of a bad
skill; it does not make running third-party instructions safe.

- **A malicious skill that passes both layers.** The deterministic rules are
  pattern-based and read one line at a time. Logic spread across files, novel
  obfuscation, or a plain-English instruction that is harmful only in your
  context will not match a rule, and the reviewer can be argued with. Approval
  means a human decided, not that the content is safe.
- **Prompt injection against your own agent.** Dalo delivers text into an agent
  folder. What your agent does when it reads that text is your agent's
  permission model, not Dalo's. A skill that passes the preflight can still try
  to steer the agent that reads it.
- **Compromised upstream hosting or maintainer accounts.** If an upstream
  repository or account is taken over, a new commit is just a new commit. Pins
  and staged audits shrink the window and make the change visible; they do not
  authenticate the author. Dalo does not verify commit signatures.
- **A compromised local machine.** Audit reports, approvals, and staged tool
  closures are local files owned by your user. Another process running as you
  can rewrite them. Dalo's guarantees end where your account's integrity ends.
- **A skill approval outliving the content it was granted for.** Unlike a tool
  or hook approval, a `skill` approval is bound to the source-qualified name,
  not to a hash. What catches a changed skill is the audit, which re-runs
  against the new content on every sync and drops any risk acceptance that no
  longer matches. An approval says "this skill may be active"; the audit is what
  keeps saying "this exact content is not blocking".
- **Your Git configuration.** Dalo drives your own `git`, so your global and
  system Git configuration still applies, and Git hooks already present in a
  managed checkout are not disabled. Dalo restricts the transports it will
  request but does not isolate Git from your machine's configuration.
- **Skills your agent writes itself.** A skill an agent drops directly into an
  agent folder is unmanaged: Dalo does not own it, does not link it, and does
  not audit it. `dalo status` reports it as an unmanaged entry, and you can
  point `dalo audit <path>` at it, but nothing scans it automatically.
- **What a skill does at runtime.** A skill that instructs an agent to install a
  package pulls in that package's supply chain. Dalo audits the skill text, not
  the ecosystem it reaches for.
- **Availability and correctness of upstream content.** Dalo reproduces what a
  pinned commit contains. It does not judge whether that content is good.

## What your team should still do

- **Review catalog pins in pull requests.** A team `dalo.toml` pins each
  external catalog to an exact revision. Treat a version bump like a dependency
  bump: read the diff of the skills it brings in, not just the version line.
- **Pin commits, not branches.** `dalo team catalog add --version <commit>` and
  `dalo team catalog version <id> <commit>` accept a full commit SHA. A branch
  name means whatever it means tomorrow.
- **Keep approvals personal.** `approvals.toml` lives in each person's own
  store. Do not copy it between machines or users — an approval is one person's
  recorded decision, and copying it discards the review it stood for.
- **Run the checks in CI.** `dalo status --check --json` fails on pending
  approvals, blocking or failed audits, lock drift, resolution diagnostics, and
  unmanaged blockers. `dalo doctor --check --json` fails on error findings such
  as a blocked security audit; pending approvals are warnings there, so run both
  rather than picking one. `dalo source refresh <catalog> --check` reports
  upstream catalog drift read-only. See [Dalo in CI](ci.md).
- **Read findings, not summaries.** When you approve a third-party skill, run
  `dalo audit <source>:<skill>` first, optionally with `--reviewer`, and read
  the findings. A short summary is the part a malicious skill can most easily
  influence.
- **Write real reasons into `--accept-risk`.** The reason is what a future
  colleague reads when the acceptance is invalidated by a change. "reviewed
  pinned upstream installer" is useful; "ok" is not.
- **Re-review on change.** Acceptances and content-addressed approvals expire on
  purpose. When one comes back, that is the system working; treat it as a
  review, not as noise.

## Related

- [SECURITY.md](../SECURITY.md) — how to report a vulnerability privately
- [`dalo audit`](reference.md#dalo-audit-skill-or-path) and
  [`dalo approve`](reference.md#dalo-approve) in the command reference
- [Troubleshooting](troubleshooting.md) — recovering from a blocked audit
- [Dalo in CI](ci.md) — the non-interactive checks

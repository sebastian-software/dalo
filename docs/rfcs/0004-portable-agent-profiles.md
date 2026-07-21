# RFC 0004: Portable Agent Profiles

Status: Draft
Date: 2026-07-20
Author: Sebastian + Codex
Depends on: RFC 0001, RFC 0002, RFC 0003

## 1. Summary

This RFC adds **agents** as Dalo's third managed asset type alongside
skills and instruction packs. An agent is a reusable role with a system prompt,
model intent, required skills, and optional capability or permission
boundaries. Dalo stores one portable canonical package, resolves it with the
same source and trust principles as other assets, and compiles it into the
native format of each linked provider.

The durable design is provider-neutral. The first implementation slice is
deliberately narrower:

- user-global agents only
- canonical Dalo packages at `agents/<name>/AGENT.md`
- local, team, and catalog source discovery
- Claude and Codex provider adapters
- explicit adoption of existing Claude and Codex agents
- agent-specific approvals and auditing
- deterministic compatibility reporting
- atomic, owned regular-file materialization

This RFC does not claim that native agent formats are interchangeable. Their
common prompt-and-metadata shape is portable, but model selection, skill
preloading, tools, permissions, hooks, and executable extensions have different
semantics and enforcement guarantees. Dalo therefore compiles from a canonical
source, reports translation fidelity field by field, and refuses a provider
projection when it cannot preserve an explicit safety boundary.

The RFC itself is the only deliverable of its first pull request. It defines no
released Rust schema or CLI behavior until the implementation milestones land.

## 2. Motivation

Skills and agents solve different problems:

- a **skill** packages reusable knowledge, procedures, and supporting files that
  an agent may load or invoke while doing work
- an **agent** selects a role and operating contract for a delegated task,
  including its system prompt, model intent, dependencies, and boundaries
- an **instruction pack** contributes standing conventions to a broader agent
  instruction file without defining a separately invocable role

An agent may require skills, but a skill does not become an agent merely because
it contains instructions. Teams benefit from managing both: skills avoid
duplicating capabilities, while named agents provide stable review, research,
implementation, or domain-specialist roles.

Provider-native agent profiles are close enough to invite sharing but not close
enough for an unqualified copy operation. Claude-style agents use Markdown with
YAML frontmatter and a prompt body. Several other coding agents follow a similar
shape. Codex uses TOML and has different configuration and enforcement
semantics. Even two Markdown consumers may disagree about tool names, model
aliases, permission modes, skill loading, hooks, or whether a field is enforced
at all.

Dalo's multi-provider promise therefore needs three layers:

```text
canonical Dalo agent package
            |
            v
resolution + approval + audit
            |
            v
provider compiler + compatibility report
            |
            v
owned native Claude/Codex agent files
```

This avoids choosing one provider's complete schema as Dalo's public contract,
while retaining a familiar Markdown authoring experience and preserving native
extensions when a provider needs them.

## 3. Goals

- Define a human-authorable, versioned canonical agent package.
- Manage agents from local, team, and catalog sources.
- Resolve agents deterministically without coupling their names to skill names.
- Let one canonical winner compile to every compatible linked provider.
- Preserve provider-native extensions without pretending they are portable.
- Make every translation loss or enforcement gap visible.
- Block only the unsafe provider projection when another projection is safe.
- Keep existing native files safe through explicit adoption and strict
  generated-file ownership.
- Extend Dalo's approval, audit, lock, status, doctor, sync, and dry-run models
  without silently broadening existing trust.
- Establish a first global Claude/Codex slice that can be expanded through
  additional provider adapters later.

## 4. Non-Goals

- Project-scoped agent targets or automatic project-profile switching.
- Cursor, Gemini, OpenCode, GitHub Copilot, OpenClaw, Hermes, or generic agent
  adapters in the first slice.
- Remote agent discovery or invocation through A2A or OASF.
- MCP server distribution or secret management.
- Workflow DAGs, agent teams, or orchestration semantics.
- Lossless arbitrary roundtripping between every pair of native formats.
- Provider-independent exact model names or tool names.
- Exposing arbitrary agent-package support files to native providers in the
  first slice.
- Weakening a portable safety constraint merely to make a provider compile.

## 5. Terms

**Canonical Agent**
The provider-neutral Dalo package whose entry point is `AGENT.md`.

**Native Agent**
An agent file in a provider's own format and discovery path, whether managed by
Dalo or unmanaged.

**Agent Package**
The directory containing `AGENT.md` and any additional reserved support files.

**Agent Slot Name**
The portable `name` and directory name. It is the global conflict key among
agent candidates.

**Agent Source Ref**
A source-qualified reference in `<source-id>:<agent-name>` form.

**Provider Adapter**
The bounded parser, compiler, and capability description for one native agent
format.

**Projection**
The compiled native representation of one resolved canonical agent for one
linked provider.

**Provider Overlay**
Provider-native values under `providers.<provider>` that supplement portable
fields for that provider only.

**Safety Boundary**
An explicit restriction on tools, filesystem access, network access, or another
capability. A safety boundary is not satisfied by prompt guidance alone.

**Compatibility Finding**
A field-level `exact`, `mapped`, `guidance_only`, `unsupported`, or `blocked`
translation result.

**Managed Asset**
The umbrella term for a skill, instruction pack, or agent. RFC 0001 used
**Agent Asset** for this role; new documentation and interfaces use **Managed
Asset** to avoid confusing the umbrella with the agent type introduced here.
This terminology change does not rename existing persisted fields.

## 6. Canonical Agent Package

### 6.1 Layout

Every source uses the same layout:

```text
agents/
  code-reviewer/
    AGENT.md
    ...                    # reserved support files
```

The local source path is:

```text
~/.dalo/local/agents/<name>/AGENT.md
```

`AGENT.md` is the only entry point. The first slice discovers only the exact
`agents/<name>/AGENT.md` shape; it does not search arbitrary directories for
agent-looking files.

The singular `AGENT.md` name is Dalo's canonical agent-profile entry point. It
is unrelated to provider instruction files named `AGENTS.md`, which contribute
repository or directory guidance and do not define an independently invocable
agent profile.

Additional files below the package directory are included in its content hash
and audit snapshot. They are not copied into Claude or Codex agent directories,
and native prompts must not assume that providers can open them. Reusable
supporting knowledge, scripts, examples, and templates should live in a required
skill until support-file projection is specified.

### 6.2 Document shape

`AGENT.md` is UTF-8 Markdown with YAML frontmatter. The first line and the
closing frontmatter delimiter are exactly `---`. The Markdown following the
closing delimiter is the canonical system prompt and must contain non-whitespace
text.

Example:

```markdown
---
schema_version: 1
name: code-reviewer
description: Reviews changes for correctness, security, and regressions.
id: example.code-reviewer
owners:
  - example:platform
tags:
  - review
  - security
targets:
  - claude
  - codex
model:
  profile: balanced
skills:
  - pr-review
tools:
  allow:
    - read-files
    - search-files
filesystem:
  read:
    - workspace
  write: []
network:
  allow: []
providers:
  claude:
    model: sonnet
---
Review the requested change. Prioritize concrete correctness and security
findings, cite the affected locations, and do not modify files.
```

### 6.3 Required fields

`schema_version`
An integer schema version. The first slice accepts exactly `1`; an unsupported
version is a blocking parse error.

`name`
The agent slot name. It must match `^[a-z0-9]+(?:-[a-z0-9]+)*$` and must equal
the package directory name byte for byte. Lowercase ASCII letters, digits, and
single separating hyphens are therefore portable; underscores, dots, uppercase
letters, leading or trailing hyphens, and repeated hyphens are rejected.

`description`
A non-empty human-facing description used by provider discovery and Dalo's CLI.
It is metadata, not a substitute for the Markdown prompt.

### 6.4 Optional portable fields

`id`
Stable identity metadata used for move detection and references. The slot name,
not `id`, remains the materialization conflict key.

`owners` and `tags`
String lists used for inventory, broad approval matching, and discovery. They do
not become provider permissions.

`targets`
An allowlist of provider IDs. When omitted, the agent targets all compatible
linked providers. An empty list targets none and leaves the agent resolved but
unlinked. Unknown syntactically valid provider IDs are retained and reported as
unsupported until an adapter exists. This field restricts projection only; it
does not participate in winner selection. In particular, a winning agent with
an empty `targets` list still shadows lower-priority same-name candidates; Dalo
does not select a different winner merely to obtain a linkable projection.

`model.profile`
One of `inherit`, `fast`, `balanced`, or `deep`. It expresses intent rather than
a provider model identifier. The default is `inherit`.

`skills`
A list of required skill references. Reference matching follows RFC 0003 §4.1:
stable IDs, source-qualified refs, and same-source relative slot names are
supported. These are hard dependencies, not suggestions.

`tools.allow`
A hard upper bound on tool capabilities. The first portable vocabulary is
`read-files`, `write-files`, `search-files`, `run-shell`, `fetch-web`,
`search-web`, `use-mcp`, and `delegate`. Omission means that this canonical
profile declares no portable tool boundary. An explicit empty list denies all
tools. Unknown portable capability names are rejected rather than ignored.

`filesystem.read` and `filesystem.write`
Hard allowlists of filesystem scopes. The first portable anchor is `workspace`,
optionally followed by a normalized relative path below that anchor. Absolute
paths, parent traversal, empty segments, and platform-specific home-directory
expansion are not portable and are rejected. Omission declares no portable
filesystem boundary; an explicit empty list denies that access mode.

`network.allow`
A hard allowlist of host names. An explicit empty list denies network access;
`"*"` allows any host. Values containing URL paths, credentials, or invalid host
syntax are rejected. An exact host name matches only that host, not its
subdomains. The first slice rejects partial wildcard forms such as
`*.example.com`; the lone `"*"` value is the only wildcard. Omission declares no
portable network boundary.

`providers.<provider>`
A provider-native mapping used for exact model selection and native extensions.
The matching adapter preserves syntactically representable values. Values for
other providers have no effect on the current projection.

Portable safety fields use omission deliberately: an absent constraint is not
the same as an empty deny-all constraint. Compilers and serializers must retain
that distinction.

### 6.5 Provider overlay rules

Provider overlays are applied after portable fields are mapped. They may:

- select an exact provider model instead of the portable profile mapping
- supply native metadata or features with no portable equivalent
- retain native values discovered during adoption
- strengthen a portable safety boundary

They may not replace the canonical name, description, or prompt, and they may
not add a capability denied by `tools.allow`, widen an explicit filesystem or
network allowlist, disable an equivalent native sandbox, or otherwise weaken a
portable safety boundary. A known weakening is `blocked`. An overlay whose
effect on a declared safety boundary cannot be determined is also `blocked` for
that provider.

Known native keys are parsed and normalized by the adapter. Unknown native keys
are retained so adoption does not discard data, included in hashing and audit,
and re-emitted only when the adapter can represent them without colliding with a
Dalo-owned output field. An unrepresentable value makes adoption or compilation
fail explicitly; it is never silently dropped.

### 6.6 Bounded parsing and package hashing

The parser is intentionally bounded:

- `AGENT.md` must be valid UTF-8 and at most 1 MiB
- YAML frontmatter is at most 64 KiB with at most 64 flow-nesting levels
- duplicate YAML keys, custom tags, aliases, and merge keys are rejected
- unknown portable top-level or nested keys are rejected; unknown keys are
  allowed only inside `providers.<provider>`
- a package contains at most 256 entries, at most 16 directory levels, and at
  most 16 MiB of regular-file content
- symlinks, sockets, devices, and paths escaping the package are blocking
  inventory findings

The content hash covers every regular package file, including `AGENT.md`, in
lexicographic relative-path order. Each record includes the normalized relative
path, file kind, byte length, and exact bytes. The hash excludes source-control
metadata outside the package. Identical package trees therefore yield identical
hashes on macOS and Linux; line endings are not rewritten before hashing.

These bounds are public schema behavior. They may be raised compatibly, but a
future implementation must not silently truncate content and continue.

## 7. Inventory and Source Behavior

Local, team, and catalog source inventories gain an `agents` collection
alongside skills and instruction packs. Each valid record contains at least:

- source ID and source-qualified ref
- slot name and optional stable ID
- package and `AGENT.md` paths
- description, owners, tags, and requested targets
- required skill references
- portable constraint summary
- content hash and source commit when applicable

Malformed packages remain visible as typed inventory diagnostics and cannot
participate in resolution. Two agent records with the same slot name inside one
source are an invalid source inventory for that slot; neither is activated.
Same-name agents in different sources are normal resolution candidates.

Dirty-source behavior matches skills. Dalo does not compile uncommitted team or
catalog agent changes during scheduled or non-interactive sync. Local packages
may be edited normally, remain visible as local state, and are still audited
before projection.

## 8. Resolution and Required Skills

Agents resolve as an independent asset type. A skill and an agent may share a
name because they occupy different namespaces and native targets.

For agents, RFC 0003's deterministic source ordering, approval, shadowing,
dirty-source, and diagnostic principles apply with these additions:

1. group approved agent candidates by agent slot name
2. choose one global winner using `(priority asc, source_id asc)`
3. leave an unapproved would-be winner pending without suppressing an approved
   lower-priority winner
4. mark all non-winning approved candidates as shadowed by the global winner
5. compute provider projections only after one winner is selected

`targets` never creates provider-specific winners. If `local:reviewer` wins, a
lower-priority `team:reviewer` cannot supply the Codex projection while the
local winner supplies Claude. Provider differences for one winner belong in its
overlays. This keeps status and lock semantics reproducible across machines with
different linked providers.

### 8.1 Required-skill closure

Every `skills` entry is required. Dependency processing occurs after both asset
inventories are available and before provider compilation:

- a same-source or same-catalog relative reference participates in required
  closure expansion
- approving or selecting a catalog agent can therefore offer a required skill
  from that catalog even when it was not otherwise selected in `dalo.toml`
- the expanded skill remains subject to the existing skill approval model
- stable-ID and source-qualified cross-source references are checked but never
  auto-selected across sources
- a missing, malformed, pending-approval, shadowed-but-not-equivalent,
  same-name-blocked, or unlinked required skill blocks the affected agent
  projection

Required skills must also be linked into the provider's skill target. It is not
enough for the skill to exist in the store. This matters for Codex, where the
first adapter can reference a skill through prompt guidance but cannot natively
preload it.

A dependency failure blocks only projections that need the unavailable skill
target. Unrelated agents and safe provider projections continue to reconcile.

## 9. Compatibility Model

Every canonical field receives one compatibility result per provider:

| Result | Meaning |
| --- | --- |
| `exact` | The native output retains the field's semantics directly. |
| `mapped` | A deterministic native representation enforces the same portable contract through different syntax or provider concepts. |
| `guidance_only` | Dalo can preserve the behavioral intent only as prompt text; the provider does not enforce it. |
| `unsupported` | The provider cannot use this non-safety metadata or optional behavior, so Dalo omits it and warns. |
| `blocked` | Compilation or materialization is unsafe or invalid; this provider projection is not written. |

`guidance_only` and `unsupported` are visible degradation warnings. An explicit
permission, tool restriction, filesystem constraint, network constraint, or
other safety boundary may be only `exact`, `mapped`, or `blocked`; prompt
guidance is not enforcement.

Compatibility is value-dependent. For example, a provider may map a
workspace-only filesystem mode but block a narrower path allowlist. Reports
therefore list the field path, result, explanation, native mapping when any, and
remediation. For deterministic aggregation, result severity increases in this
total order:

```text
exact < mapped < guidance_only < unsupported < blocked
```

The overall provider result is the greatest field result in that order.
Human-facing output may suppress a warning for Dalo-only identity metadata that
is intentionally `unsupported`, as described in §10.4, without changing its
machine-readable classification or aggregation order.

A `blocked` Codex projection does not suppress a safe Claude projection of the
same canonical winner, and vice versa. `sync` applies safe operations, reports
blocked projections, and uses the existing unsafe-state exit behavior so
automation can detect partial materialization.

## 10. Provider Adapters

### 10.1 Adapter contract

Each provider adapter owns:

- standard global skill and agent paths
- native filename and syntax validation
- deterministic canonical-to-native compilation
- native-to-canonical adoption parsing
- model-profile and portable-capability mappings
- native fields it owns and overlay fields it may re-emit
- compatibility classification and safety enforcement claims
- canonical ordering and escaping rules for snapshot-stable output

An adapter must not claim enforcement based only on language in the prompt. New
provider adapters require an RFC amendment or follow-up RFC that publishes their
mapping and acceptance matrix.

The first-slice contracts in this RFC were verified on 2026-07-21 against
[Claude Code subagents](https://code.claude.com/docs/en/sub-agents) as exposed by
Claude Code 2.1.207 and [Codex
subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents) as
exposed by Codex CLI 0.144.2. These versions are adapter verification baselines,
not maximum supported versions. Snapshot tests record verified versions, while
`doctor` reports an installed provider version without a matching baseline as
unverified. Such a version is not rejected solely because of its version, but
any change to discovery paths, required keys, parsing, or enforcement claims
requires an adapter-contract and acceptance-snapshot update before Dalo relies
on the changed behavior.

### 10.2 Claude

The first Claude adapter writes:

```text
~/.claude/agents/<name>.md
```

The generated file contains Claude YAML frontmatter followed by the canonical
Markdown prompt. Dalo owns the whole generated file, not a block within it.

Portable model profiles map deterministically:

| Profile | Claude model value |
| --- | --- |
| `inherit` | inherited/default model |
| `fast` | `haiku` |
| `balanced` | `sonnet` |
| `deep` | `opus` |

An exact `providers.claude.model` value overrides this profile mapping. Required
skills map to Claude's native skill preload metadata and are `exact` when every
required skill is linked. Portable tool capabilities map to the corresponding
Claude tool names only when the resulting native allowlist is enforceable.

Filesystem and network constraints are mapped only when the current Claude
profile format and runtime provide an enforceable equivalent. Otherwise the
projection is blocked; omitting a tool from an allowlist is not treated as a
path-level sandbox unless the runtime guarantees that equivalence.

### 10.3 Codex

The first Codex adapter writes:

```text
~/.codex/agents/<name>.toml
```

The canonical Markdown body is rendered as the TOML
`developer_instructions` value. Description and other supported native metadata
use canonical TOML key order and deterministic multiline-string escaping.

Portable model profiles keep the inherited model and map reasoning effort:

| Profile | Codex mapping |
| --- | --- |
| `inherit` | inherited model and reasoning effort |
| `fast` | inherited model, low reasoning effort |
| `balanced` | inherited model, medium reasoning effort |
| `deep` | inherited model, high reasoning effort |

Exact model or reasoning values belong in `providers.codex`. A required skill is
rendered into `developer_instructions` as deterministic usage guidance and is
reported as `guidance_only`; the required skill must still be linked into the
Codex skill path.

The first Codex adapter cannot enforce a portable hard tool allowlist. Any
explicit `tools.allow`, including an empty list, therefore blocks Codex
projection. Filesystem and network constraints compile only when a native Codex
sandbox setting enforces the requested boundary; non-equivalent constraints
block rather than degrade to instructions.

### 10.4 Initial field baseline

The exact result still depends on the value, but adapters start from this
published baseline:

| Canonical field | Claude | Codex |
| --- | --- | --- |
| `name`, `description`, Markdown prompt | `exact` | `exact` |
| `id`, `owners`, `tags` | `unsupported` native metadata; retained by Dalo | `unsupported` native metadata; retained by Dalo |
| `targets` | `exact` in Dalo; not emitted | `exact` in Dalo; not emitted |
| `model.profile` | `mapped` | `mapped` |
| exact provider model overlay | `exact` | `exact` |
| required `skills` | `exact` native preload | `guidance_only` |
| hard `tools.allow` | `mapped` when enforceable, otherwise `blocked` | `blocked` |
| filesystem/network constraints | `mapped` when enforceable, otherwise `blocked` | `mapped` when enforceable, otherwise `blocked` |
| matching provider-native overlay | `exact`, `unsupported`, or `blocked` by key | `exact`, `unsupported`, or `blocked` by key |
| additional package files | `unsupported` | `unsupported` |

The compiler emits no warning merely because Dalo-only identity metadata is not
copied into a native file, but JSON compatibility data still records the
classification. Unsupported behavioral fields and support files produce a
human-visible warning.

## 11. Provider-Aware Targets

One linked built-in target represents a complete provider rather than one asset
directory. Its state may contain separate skill, agent, and instruction paths.

Initial defaults are:

| Target | Skill path | Agent path | Agent adapter |
| --- | --- | --- | --- |
| `claude` | `~/.claude/skills` | `~/.claude/agents` | yes |
| `codex` | `~/.agents/skills` | `~/.codex/agents` | yes |
| `openclaw` | `~/.agents/skills` | none | no |
| `hermes` | `~/.hermes/skills` | none | no |
| `generic` | required custom path | none | no |

`dalo target link claude` and `dalo target link codex` link both standard asset
paths. `target unlink` removes only owned materializations whose ownership checks
pass, then removes the complete logical target record. Skill-only targets retain
their current behavior.

The existing optional positional path remains a skill-path override for backward
compatibility:

```text
dalo target link claude /custom/claude/skills
```

Named overrides make custom installations unambiguous:

```text
dalo target link claude --skills-path /custom/skills \
  --agents-path /custom/agents
```

Passing both the legacy positional path and `--skills-path` is invalid. A custom
`--agents-path` does not imply a custom skill path. `generic` continues to
require its positional or named skill path and rejects `--agents-path` until it
has an agent adapter.

### 11.1 State migration

On the first state-file migration supporting agents:

- each existing linked `claude` target gains `~/.claude/agents`
- each existing linked `codex` target gains `~/.codex/agents`
- its existing path remains the skill path, including a previous custom
  positional override
- no native agent file is adopted, overwritten, removed, or marked owned
- other target kinds remain skill-only

Migration is deterministic and idempotent. It may create the logical path value
in state, but target directories are created only by a real non-dry-run
materialization plan. Any unmanaged file found there is reported as a conflict.

`target detect`, `target link`, `target unlink`, `status`, `doctor`, and their
JSON reports expose asset-specific paths and adapter availability.

## 12. CLI Surface

The first slice adds:

```text
dalo agent list
dalo agent show <source>:<name> [--provider <id>]
dalo agent adopt <path-or-native-name> [--provider claude|codex] [--replace]
```

`agent list` shows candidates, the selected winner, approval state, required
skills, targeted providers, and per-provider projection status. `agent show`
shows canonical metadata and prompt provenance; `--provider` adds the compiled
preview and compatibility findings without writing it. Secret-looking overlay
values are redacted from normal text and JSON output.

`agent adopt` accepts either an explicit native file path or a native agent
name. A name requires `--provider`; Dalo resolves it through that provider's
linked or default global target. For a path, `--provider` is optional only when
the path belongs to exactly one linked or default provider agent root and that
adapter accepts the file. Otherwise the caller must supply it. Provider-like
`claude:<name>` and `codex:<name>` operands are not special adoption syntax:
colon-qualified values remain source refs elsewhere in the CLI, and source IDs
remain free to use `claude` or `codex`. Adoption never scans and adopts all
native agents implicitly.

Existing commands change as follows:

- `sync` resolves, audits, compiles, and reconciles targeted agents together
  with existing asset types
- `status` and `doctor` include agent inventory, approval, dependency,
  compatibility, drift, and materialization findings
- `audit <source>:<name> --asset agent` uses agent-specific rules; the asset
  selector is explicit because the same source may contain a same-name skill
- `resolve list` exposes exact agent projection IDs, while `resolve
  restore-owned` and `resolve remove-owned` accept `--asset agent` for the drift
  recovery behavior in §16
- the existing optional semantic-review selector is renamed from `--agent` to
  `--reviewer`; `--agent` remains a deprecated compatibility alias for at least
  one release cycle, preserves its existing behavior, and conflicts with an
  explicitly supplied `--reviewer`
- JSON reports add typed agent collections without reusing skill record types
- every new mutating path supports `--dry-run`

Existing skill references and command forms retain their meaning. Where a
generic command accepts both asset types, `--asset skill|agent` resolves
ambiguity; existing skill-only forms default to `skill` for backward
compatibility. `dalo agent ...` and `dalo approve agent ...` are already
unambiguous and need no asset flag. For example, semantic review of an exact
agent approval is requested as
`dalo approve agent team:reviewer --reviewer codex`, avoiding the ambiguous
`approve agent ... --agent ...` form.

## 13. Approval and Trust Model

Agent activation is a separate trust decision because an agent's prompt and
permissions may be more powerful than any individual required skill.

- local agents are eligible for activation without an approval record
- team and catalog agents require agent-scoped approval
- every projection, including a local one, must still pass deterministic audit
  and compatibility checks
- scheduled and non-interactive sync never grant approval
- approving an agent does not approve its required skills
- approving a skill does not approve a same-name agent

The exact command is:

```text
dalo approve agent <source>:<name>
```

Source, author, and organization approvals gain an asset selector whose default
remains `skill`:

```text
dalo approve source team --asset agent
dalo approve author public:maintainers --asset agent
dalo approve org public:example-org --asset agent
```

Revocation supports the corresponding agent scope and asset-qualified broad
records. Existing approval files migrate every skill, source, author, and org
record as `asset = "skill"`. No legacy record can authorize an agent merely
because its value matches. Broad agent approvals remain visible as broad trust
in `status` and `doctor`.

Exact agent approval runs deterministic audit first and supports the same
optional isolated semantic reviewer and content-hash risk acceptance model as
skill approval. A risk acceptance applies only to the audited canonical package
hash and provider overlay set. It does not override a compiler's inability to
enforce a safety boundary, an unmanaged-file conflict, or generated-file drift.

## 14. Agent Audit

Agent packages reuse the existing bounded file walk, content-hash cache,
isolated semantic reviewer, and severity model. Deterministic rules additionally
inspect:

- prompt-injection patterns that redirect or conceal the agent's contract
- requests to discover, print, transmit, or persist secrets
- inline or provider-overlay MCP server definitions, especially executable
  commands and environment values
- hooks, shell commands, startup commands, and command substitution
- permission bypasses, broad bypass modes, and requests to disable sandboxing
- absolute, escaping, sensitive, or world-writable filesystem paths
- unrestricted or suspicious network destinations
- tool aliases or provider overlays that widen portable capabilities
- hidden behavior in support files that the canonical prompt references
- unknown native keys whose security effect cannot be classified

Findings identify the canonical field or package file and, when relevant, the
affected provider. Provider-specific blocking findings suppress only that
projection. Findings in the shared prompt or package can block all projections.

Audit output must distinguish:

- deterministic content risk that may be accepted for one exact hash
- semantic-review advice that may be accepted for one exact hash and reviewer
  contract
- structural invalidity, unsafe path traversal, enforcement incompatibility,
  or ownership conflict that cannot be bypassed by risk acceptance

No-finding audit results remain best-effort observations, not a safety
certification, matching the existing skill audit contract.

## 15. Catalog Agents and Drift

Configured catalog checkouts discover every valid canonical
`agents/*/AGENT.md`. The first slice adds no `agents` property or include/exclude
syntax to `dalo.toml`.

All discovered catalog agents are offered as `pending_approval`. The user's
agent approval state selects which candidates can become active. This keeps the
team manifest stable while the agent format and curation needs are still new.
An explicit broad agent approval may cover future catalog agents, but is shown
as broad trust and remains separate from all legacy skill approvals.

The expected first-slice curation flow is intentionally user-selected: a
catalog operator publishes canonical packages, refresh exposes them as
`new_available` or `pending_approval`, the user inspects candidates with
`dalo agent list` and `dalo agent show`, and exact `dalo approve agent ...`
records activate the chosen agents. A team may recommend a set through its
catalog and documentation, or explicitly grant a broad agent approval, but the
team manifest cannot silently activate individual catalog agents in this slice.
Finer manifest-level agent selection is deferred until usage establishes the
needed curation semantics.

Catalog snapshots record agent stable ID, slot name, relative path, content
hash, required skills, and source commit. Refresh and drift reporting classify:

- `new_available`: a newly discovered, still-pending agent
- `changed`: the package hash of a known agent changed
- `moved`: a stable-ID agent moved to a different canonical path
- `removed`: a known agent disappeared

A removed approved agent cannot be projected from stale inventory after its
source update. A changed approved agent is re-audited and subject to the
content-hash approval and risk-acceptance rules. A broken required-skill closure
blocks the dependent projection. Safe unrelated skills, instruction packs,
agents, and provider projections continue to reconcile.

## 16. Safe Generated-File Materialization

Agent projections are atomic regular files, not symlinks. The canonical and
native formats differ, and native agent discovery does not consistently follow
symlinks.

For each provider projection, reconciliation compares:

- **D (desired):** compiled bytes and their generated hash
- **R (recorded):** Dalo's ownership record and last materialized hash
- **A (actual):** the target file's current bytes and hash, or absence

The rules are:

| Desired | Recorded | Actual | Action |
| --- | --- | --- | --- |
| yes | yes | hash equals desired and last materialized | no-op |
| yes | yes | hash equals last materialized, desired changed | atomically update |
| yes | yes | absent | recreate and report externally removed ownership |
| yes | yes | hash equals desired but differs from last materialized | `converged`; write no file, report state recovery, and repair the recorded hashes after validation |
| yes | yes | any other hash differing from last materialized | `drift`, block and never overwrite implicitly |
| yes | no | absent | atomically create and record ownership |
| yes | no | any file or symlink | unmanaged conflict; never overwrite |
| no | yes | hash equals last materialized | safely remove and drop ownership |
| no | yes | absent | drop stale ownership record |
| no | yes | hash differs from last materialized | drift; leave file and drop nothing |
| no | no | anything | ignore |

The `converged` case is safe to recover because the current bytes already equal
the newly compiled desired bytes. Before repairing state, Dalo repeats canonical
validation, dependency preflight, audit, and provider compilation and confirms
the actual hash from a stable snapshot. It then updates the source, generated,
and last-materialized hashes without rewriting the native file.

Drift always has an explicit remediation path:

```text
dalo resolve restore-owned <target>:<slot> --asset agent
dalo resolve remove-owned <target>:<slot> --asset agent
```

`restore-owned` requires an existing ownership record and desired projection.
It repeats validation, audit, compatibility, and stable actual-file checks, then
atomically replaces the drifted file with the current compiled output and
updates state. `remove-owned` preserves a drifted regular file and drops only
its ownership record; if the projection remains desired, later sync reports the
preserved file as an unmanaged conflict. For an unchanged owned file, the
existing safe-deletion behavior remains. Both commands support `--dry-run`, and
existing skill-only forms continue to default to `--asset skill`. Importing
native drift back into an existing same-name canonical package remains an
explicit non-goal of the first slice rather than an implicit merge.

A path is owned only when its provider, canonical target path, and generated
hash are recorded in `state.toml`. Filename appearance or matching content alone
never establishes ownership. Symlinks at an agent-file path are treated as
unmanaged conflicts even when they point inside the Dalo store.

Creation and update write a same-directory temporary file, set safe permissions,
flush it, and atomically rename it where the platform permits. The operation
plan records rollback data before apply. If a later file or state write in the
transaction fails, Dalo restores the prior owned bytes or removes the newly
created file, then restores the prior state. Recovery metadata makes an
interrupted operation diagnosable and resumable on the next run.

New Dalo-created agent directories use user-only permissions and generated
agent files use mode `0600`. Dalo does not relax permissions on an existing
provider directory.

State is committed only for operations confirmed on disk. A second sync with
unchanged inputs produces only no-ops. `--dry-run` builds and reports the same
compile, audit, compatibility, ownership, conflict, deletion, and rollback plan
without creating directories, temporary files, backups, or state changes.

## 17. Native Adoption

Adoption is explicit and provider-aware:

1. identify one Claude Markdown or Codex TOML native agent
2. read a bounded, stable snapshot and hash it
3. parse known fields through the provider adapter
4. normalize name, description, prompt, model intent, skills, and enforceable
   constraints into the canonical schema
5. retain remaining native values under `providers.claude` or
   `providers.codex`
6. validate, hash, compile, and audit the resulting canonical package
7. write the canonical package to `~/.dalo/local/agents/<name>/AGENT.md`

Native exact models remain provider overlays unless they match a portable model
profile without loss. Unknown values are preserved, not guessed. A native agent
missing a required canonical field, using a non-portable name, containing an
unrepresentable value, or racing with an on-disk change fails with an actionable
error.

Without `--replace`, adoption copies the normalized agent into the local source
and leaves the native file unmanaged. That file then blocks Dalo's projection to
the same path while other safe providers may still materialize.

With `--replace`, Dalo may replace only the exact native file that was adopted.
Before touching it, Dalo:

- refuses if any canonical agent with the same name already exists
- verifies the native file still matches the initial hash
- stores a byte-exact recovery copy in Dalo's adoption backup area
- completes canonical validation, provider compilation, dependency preflight,
  audit, and compatibility checks
- durably persists the canonical local package and a prepared ownership
  transaction

It then atomically installs the compiled native output and commits ownership.
Any failure restores the original native bytes and previous state. Successful
replacement retains the byte-exact recovery copy; automatic backup pruning is
deferred. Dalo never relies on the normalized canonical package as the only
backup.

Adoption never implicitly merges with an existing canonical same-name agent and
never bulk-adopts a directory merely because `target link` or `sync` sees native
files.

## 18. Persisted Models and Reports

The user lock gains agent collections parallel to, but type-distinct from,
skills:

- active agents with source ref, stable ID, source commit, and package hash
- pending-approval and shadowed agent candidates
- required skill refs and resolved dependency identities
- targeted providers and deterministic compatibility summaries
- catalog path and snapshot identity when applicable

`state.toml` records each owned projection's:

- provider ID
- canonical target path
- source-qualified agent ref
- canonical source/package hash
- current desired generated hash
- last successfully materialized hash
- recovery or prepared-transaction metadata when needed

Generated bytes do not become canonical source and are not written into
`lock.toml`. A machine with different linked providers resolves the same global
winner but may have different state and compatibility outcomes.

Text and JSON reports distinguish at least:

- `active`, `pending_approval`, `shadowed`, `blocked`, and `unlinked` agent state
- per-provider `not_targeted`, `target_unlinked`, `ready`, `degraded`, `blocked`,
  `conflicted`, `drifted`, and `materialized` projection state
- field-level compatibility findings
- required-skill closure and exact blocker refs
- owned and unmanaged native paths
- planned/applied/no-op/rolled-back operations

JSON uses explicit schema-versioned agent and projection record types. Existing
skill fields keep their released meaning; no existing array silently starts
containing agents.

## 19. First Implementation Slice

Implementation should proceed in five reviewable stages after this RFC is
accepted.

### 19.1 Canonical model and compilers

- add canonical schema types and validation
- implement the bounded parser and package content hash
- extend local, team, and catalog inventories with agent records
- implement deterministic Claude and Codex compilation
- snapshot native bytes, verified provider-version baselines, and field-level
  compatibility reports

This stage writes no provider target files.

### 19.2 Provider targets and ownership

- evolve logical target state to asset-specific provider paths
- automatically migrate existing Claude and Codex target records
- implement atomic regular-file planning and apply
- add ownership hashes, convergence recovery, drift detection, explicit restore
  or relinquish remediation, safe deletion, recovery, and rollback
- make every operation available through dry-run before enabling writes

### 19.3 Resolution, approval, dependencies, and audit

- add agent winner selection and shadowing
- add agent-scoped approval records and migration of legacy approvals to skill
- rename the semantic reviewer selector to `--reviewer` with the deprecated
  compatibility alias described in §12
- implement required-skill closure and provider-link preflight
- add agent deterministic and semantic audit integration
- extend lock and status domain models

### 19.4 User-facing resolution and sync

- integrate agents into `sync`, `status`, and `doctor`
- add `agent list` and `agent show`
- publish stable text and JSON compatibility reporting
- harden partial per-provider blocking behavior

### 19.5 Adoption, catalogs, and hardening

- add native Claude and Codex adoption with transactional `--replace`
- add catalog snapshots and agent drift reporting
- document authoring, approval, troubleshooting, and migration
- complete end-to-end, interruption, and cross-platform tests

No stage may silently enable partially implemented agent projection. Feature
gates or schema-version checks should keep an incomplete path read-only until
its safety and rollback tests pass.

## 20. Acceptance Matrix

Canonical parsing and inventory:

- valid minimal and full `AGENT.md` packages parse
- missing frontmatter, required fields, or prompt are rejected
- malformed YAML, duplicate keys, unknown portable keys, and unsupported schema
  versions are rejected
- invalid names and name/path mismatches are rejected
- `AGENT.md` discovery never treats an `AGENTS.md` instruction file as an agent
  package
- file, frontmatter, package-size, entry-count, and depth limits are enforced
- package symlinks and escaping paths block inventory
- duplicate names inside one source activate neither record
- package hashes are stable across repeated scans and platforms

Compilation and compatibility:

- identical canonical input produces byte-identical Claude Markdown and Codex
  TOML
- multiline prompts and native overlay strings are escaped deterministically
- portable model profiles map exactly as published
- adapter snapshots record the Claude Code and Codex CLI verification baselines,
  and `doctor` reports installed versions without a matching baseline as
  unverified
- exact provider model overlays take precedence
- provider-native overlays survive adopt/compile when representable
- overlays cannot replace canonical identity/prompt or weaken safety boundaries
- reports cover `exact`, `mapped`, `guidance_only`, `unsupported`, and `blocked`
- aggregate compatibility uses the published total severity order
- a blocked provider does not suppress another safe provider projection
- Codex required skills produce guidance and a visible degradation
- Codex hard tool allowlists block projection

Targets, state, and materialization:

- existing Claude/Codex target records gain standard agent paths once and keep
  their skill-path overrides
- existing approvals are explicitly skill-scoped after migration
- linking a built-in provider exposes both supported asset paths
- skill-only providers reject agent path configuration
- unmanaged native files are never overwritten by sync
- owned files update or delete only when the actual hash matches recorded output
- actual bytes already equal to newly desired bytes recover ownership state
  without rewriting the native file
- drift, missing owned files, foreign symlinks, and orphaned state follow the
  published reconciliation table
- drifted agent files can be explicitly restored from canonical output or
  preserved while agent ownership is relinquished; neither path runs implicitly
- failed apply/state transactions roll back to consistent bytes and state
- interrupted prepared transactions are diagnosed and recoverable
- a second unchanged sync is a no-op
- dry-run covers directory creation, compile, audit, conflict, update, deletion,
  adoption, replacement, rollback, and state migration without mutation

Resolution, catalogs, and trust:

- agents resolve independently from skills and may share a name with a skill
- one global winner is used for every provider
- a winner with an empty `targets` list still shadows lower-priority same-name
  candidates
- unapproved higher-priority agents do not suppress an approved lower-priority
  winner
- local agents are eligible directly; team and catalog agents remain pending
  until separately agent-approved
- legacy skill/source/author/org approvals never authorize agents
- agent-scoped broad approvals match only the requested asset type
- catalog discovery needs no new `dalo.toml` agent filter
- exact catalog-agent curation remains user-selected unless an explicit broad
  agent approval applies
- new, changed, moved, and removed catalog agents are reported deterministically
- required-skill closure works within local, team, and catalog sources
- missing, unapproved, shadowed-incompatible, or unlinked skills block the
  affected projection
- unrelated assets and safe provider projections still reconcile
- stable text and JSON reports expose all blockers and compatibility findings

Adoption and audit:

- native Claude Markdown and Codex TOML adopt into valid canonical packages
- named native adoption requires `--provider`; provider-like colon operands are
  not overloaded as native references
- known shared fields normalize while remaining native values stay in the
  matching provider overlay
- adoption without `--replace` preserves the unmanaged native file
- adoption with `--replace` keeps a byte-exact recovery copy and becomes owned
  only after all preconditions succeed
- same-name canonical agents, native-file races, invalid native input, failed
  audit, and unenforceable safety constraints prevent replacement
- deterministic audit finds prompt injection, secrets, executable MCP values,
  hooks/shell commands, permission bypasses, unsafe paths, and widening overlays
- semantic review uses `--reviewer`; the deprecated `--agent` alias retains its
  old meaning and conflicts with an explicit `--reviewer`
- exact network hosts do not include subdomains, partial wildcards are rejected,
  and the lone `"*"` wildcard retains its published meaning
- hash-scoped risk acceptance never bypasses structural, compatibility, or
  ownership safety

## 21. Relationship to Emerging Formats and Protocols

Dalo intentionally uses a Claude-style Markdown envelope because it is familiar,
diffable, and close to several coding-agent profile formats. Its separation of a
portable core and provider extensions also aligns conceptually with Agent
Flavored Markdown and Oracle Agent Spec translation models.

This RFC does **not** claim conformance with Claude's complete schema, Agent
Flavored Markdown, Oracle Agent Spec, or any standards body's agent model. Dalo's
canonical schema remains a local coding-agent profile and packaging contract.
Future convergence should happen through versioned import/export adapters rather
than silently changing the meaning of existing fields.

A2A and OASF address remote-agent discovery, identity, communication, or runtime
interoperability rather than Dalo's local profile-authoring problem. MCP addresses
tool connectivity. They may become integration points later, but remote agents,
runtime orchestration, and MCP server lifecycle management remain out of scope.

## 22. Deferred Work

- project-scoped agent discovery and materialization
- Cursor, Gemini, OpenCode, GitHub Copilot, OpenClaw, Hermes, and generic adapters
- versioned AFM or Oracle Agent Spec import/export
- remote A2A agents and OASF discovery
- workflow DAGs and agent-team orchestration
- arbitrary native-to-native conversion and lossless roundtripping guarantees
- native projection of agent-package support files
- richer catalog curation or an `agents` manifest filter, after real usage shows
  the required selection semantics
- provider capability probing beyond the published adapter contract

## 23. RFC Validation

The documentation-only RFC pull request is complete when:

```sh
sh tests/docs.sh
git diff --check
```

both pass and no Rust schema, persisted-state version, or CLI behavior changes
are included. `tests/docs.sh` exercises existing documentation and CLI
invariants; it does not parse or directly assert this Draft RFC. Its success is
therefore a repository regression check, not proof that the proposed contract
is implemented or internally complete.

RFC review must additionally confirm the status, date, authorship, dependencies,
provider verification baseline, normative decisions, deferred scope, and
acceptance matrix in this document. Executable parser, compiler, migration,
reconciliation, and CLI contract tests become mandatory in the implementation
stages and must not be represented as covered by this RFC-only pull request.

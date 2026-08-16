# RFC 0005: Portable Plugins and Composable Agent Stacks Beyond Skills

Status: Draft
Date: 2026-07-31
Author: Sebastian + Codex
Depends on: RFC 0001, RFC 0003, RFC 0004
Related: #468, #494, #495–#503
Discussion: https://github.com/sebastian-software/dalo/issues/495

## Summary

Dalo should grow from a manager of individually resolved agent assets into a
manager of **portable plugins** and authored **agent stacks** that compose them
with standalone assets, shared instructions, sources, routing, and policy.

The existing asset types remain valid and independently manageable:

- Agent Skills packages reusable workflows and resources.
- Instruction packs contribute standing guidance to files such as `AGENTS.md`
  and `CLAUDE.md`.
- Portable agent profiles define independently invocable or delegable roles and
  compile through the provider adapters introduced by RFC 0004.

The missing abstraction is a provider-neutral way to say that a set of these
assets belongs together with local tools, lifecycle hooks, dependencies, and
fallback behavior. Dalo should preserve that authored intent, resolve it with
its existing source, lock, approval, audit, and ownership model, and project it
into each linked harness's native installation format.

A Codex plugin, Claude Code plugin, Cursor integration, or OpenCode plugin is a
**target projection** of that plugin. It is not Dalo's canonical source format.

This RFC proposes the vocabulary and architecture for that layer. It does not
make lossless cross-harness behavior a promise, and it does not propose running
newly discovered code implicitly.

## Motivation

The open [Agent Skills specification](https://agentskills.io/specification)
provides a useful portable core:

```text
skill-name/
  SKILL.md
  scripts/
  references/
  assets/
```

That core is sufficient for most instruction-heavy skills. It deliberately does
not standardize:

- standing repository or user guidance;
- subagent definitions and execution isolation;
- lifecycle hook events and output contracts;
- local executable registration;
- MCP server and connector wiring;
- plugin marketplace metadata;
- harness-specific permissions, trust, and installation sidecars.

Dalo already addresses two adjacent parts of this problem:

1. instruction packs safely render owned blocks into harness instruction files;
2. RFC 0004 defines canonical agent profiles, provider adapters, field-level
   compatibility results, and safety-preserving projections.

Real packages increasingly combine all of these parts. Impeccable is a useful
stress case:

- its design rules and most helper scripts are portable;
- its command prefixes and install paths are mechanically target-dependent;
- its reviewer, documenter, and asset roles need different native agent
  formats;
- its edit/stop detector uses different hook schemas and events across
  harnesses;
- its native metadata and plugin packaging differ by target.

The lesson is not that every skill needs a compiler. The lesson is that Dalo
needs a vocabulary for the boundary between:

1. portable content;
2. translatable capabilities;
3. target-native integration.

Without that boundary, projects either maintain many hand-authored provider
trees or build bespoke compilers that Dalo can only treat as opaque skill
directories. Issue #494 addresses target-specific skill derivations and
generated artifacts. This RFC defines the broader composition model in which
those derivations, agents, tools, and hooks can belong to one logical unit.

## Goals

- Keep standard Agent Skills byte-identical by default.
- Compose skills, instruction packs, portable agents, tools, hooks, and assets
  into one versioned logical plugin.
- Let authored agent stacks compose portable plugins, standalone assets,
  source selections, routing, and policy without becoming a provider package
  format.
- Treat Codex, Claude Code, Cursor, OpenCode, and future plugin formats as
  projection targets.
- Reuse RFC 0004's provider adapter and compatibility vocabulary.
- Make every degradation, omission, and unsupported capability inspectable.
- Require explicit trust for active code such as hooks, generators, local
  tools, and MCP servers.
- Keep Dalo's ownership boundary narrow, typed, atomic, and recoverable.
- Let provider adapters evolve without making native field names Dalo's public
  portable schema.
- Support a useful portable-only installation when native integration is
  unavailable or declined.

## Non-goals

- Lossless arbitrary conversion between provider formats.
- A universal workflow DAG or multi-agent orchestration language.
- A provider-independent vocabulary for exact model IDs or native tool names.
- Secret or credential distribution.
- Implicit execution of build scripts, hooks, package-manager lifecycle scripts,
  or MCP servers discovered in a source.
- General dotfile management.
- Replacing native provider plugin formats.
- Rewriting standard `SKILL.md` content merely to install it.
- Making every optional provider feature part of the first implementation
  slice.

## Terminology

### Managed component

One independently modeled unit:

- skill;
- instruction pack;
- agent;
- local tool;
- hook;
- MCP server declaration.

The first three already have Dalo models. Tools and hooks need new typed models.
MCP declarations and a generic static-asset component remain reserved until a
concrete lifecycle, reference grammar, materialization target, and ownership
contract exist. Existing skills, agents, and plugins may continue to carry
support files inside their own bounded packages.

Independently modeled means independently addressable for validation, hashing,
approval, audit, compatibility, and diagnostics. It does not require every
component to be independently published. In the first active-code slice, tools
and hooks are plugin-local, namespaced components.

### Portable plugin

A versioned authored unit that groups managed components and declares their
relationships. A plugin is the closest portable analogue to a provider plugin.

`Agent Package` already means the `agents/<name>/AGENT.md` directory in RFC
0004, so this RFC deliberately does not reuse that term for the larger plugin.

### Agent stack

An authored, versioned agent system that composes portable plugins, standalone
skills and agents, instruction packs, source selections, routing, fallback
rules, and policy.

The existing
[`sebastian-software/agent-stack`](https://github.com/sebastian-software/agent-stack)
repository is a concrete example: it owns pinned selections, specialized
agents, shared instructions, and orchestration rules. A stack is therefore an
architectural and publication concept, not merely the name of Dalo's resolved
runtime view.

The first slice does not require a second `STACK.toml` or named
stack-profile language. The source-composition repository and its root
`dalo.toml` are the initial authored-stack boundary. The root manifest gains a
small reference-only selection surface for portable plugins; portable-plugin
definitions remain in their bounded packages. Direct user selections are local
overlays and never rewrite authored stack intent.

Dalo describes the result after resolution, approval, and target capability
checks as the **effective configuration**, **installation plan**, or **target
state** without introducing another public domain noun.

### Target projection

The target-native representation of a resolved component or plugin. Examples:

- a Codex plugin plus managed `AGENTS.md` blocks;
- a Claude Code plugin plus managed `CLAUDE.md` blocks;
- Cursor skills, agents, rules, and hook configuration;
- a portable-only skill installation when no richer adapter is available.

### Adapter

A bounded implementation that declares target capabilities and renders owned
native artifacts. Adapters never silently claim semantic equivalence.

## Relationship to provider plugins

Codex currently defines a plugin as an installable directory with
`.codex-plugin/plugin.json` that can point to skills, MCP server configuration,
apps, lifecycle hooks, and presentation assets:
<https://developers.openai.com/plugins/build/plugins>.

That is structurally similar to a Dalo portable plugin, but it is not a
portable source format:

- paths and manifest fields are Codex-specific;
- hook schemas and trust behavior are host-specific;
- top-level instruction files remain outside the plugin;
- portable agents are not a general top-level plugin primitive;
- another harness cannot be expected to consume the same manifest.

Therefore:

```text
Dalo portable plugin  != native Codex/Claude/Cursor/OpenCode plugin
Dalo portable plugin  -> one independently owned native plugin projection
authored agent stack  -> portable plugins + standalone assets + shared policy
resolution + approval -> effective target plan and installation state
```

## Canonical source and activation contract

This section is normative for the passive first slice in #496–#498. Later
active-code issues may add optional descriptor fields, but they may not change
the identity, containment, selection, dependency, or activation rules defined
here without amending this RFC.

### Package boundary and layout

A portable plugin is exactly one bounded package at:

```text
plugins/<plugin-name>/
  PLUGIN.toml
  ...                       # plugin-owned support and future active-code files
```

Discovery checks only this exact shape below each enabled source root. It does
not recursively search for plugin-looking manifests. `<plugin-name>` is the
plugin slot name and must match `^[a-z0-9]+(?:-[a-z0-9]+)*$`.

`PLUGIN.toml` is UTF-8 TOML. The passive version-1 schema is:

```toml
schema_version = 1

[plugin]
name = "impeccable"
id = "dev.impeccable.plugin"
description = "Frontend design, review, and quality workflows."
version = "1.0.0"

[[plugin.members]]
ref = "skill:impeccable"
requirement = "required"

[[plugin.members]]
ref = "agent:impeccable-finish-reviewer"
requirement = "optional"

[plugin.members.fallback]
kind = "inline"
skill = "skill:impeccable"

[[plugin.members]]
ref = "instruction:engineering-defaults"
requirement = "recommended"

[[plugin.requires]]
ref = "plugin:company-engineering"
requirement = "required"

[providers.codex]
interface = "providers/codex/interface.toml"
```

The fields have these meanings:

| Field | Contract |
| --- | --- |
| `schema_version` | Required integer. The first slice accepts exactly `1`. |
| `plugin.name` | Required slot name; it equals the package directory byte for byte. |
| `plugin.id` | Optional stable move-detection identity, unique among plugins in one source. |
| `plugin.description` | Required non-empty human-facing description. |
| `plugin.version` | Optional informational authored version. Resolution still pins source provenance and hashes. |
| `plugin.members[]` | Ordered declarations of existing passive managed components. Order is retained for display only and never changes resolution. |
| `plugin.requires[]` | Plugin dependency declarations. Dependencies do not imply approval. |
| `providers.<provider>` | Optional provider-native overlay owned and validated by that adapter. |

`plugin.id`, when present, is 1–128 ASCII characters matching
`^[a-z0-9]+(?:[.-][a-z0-9]+)*$`. It cannot contain `:`, `#`, `/`, or `\\`.
Two packages in one source with the same stable ID are both invalid inventory
candidates even when their slot names differ. Stable IDs support move detection
and exact references; slot names remain conflict and native-package keys.

Version 1 accepts these passive member kinds:

| Reference kind | Allowed requirement | Activation consequence |
| --- | --- | --- |
| `skill` | `required`, `optional` | Becomes desired resolver input; existing content approval and audit still apply. |
| `agent` | `required`, `optional` | Becomes desired resolver input; agent-specific approval and audit still apply. |
| `instruction` | `required`, `optional`, `recommended` | Never enables a block. It only checks or recommends an independently enabled instruction pack. |

`requirement` is mandatory; there is no implicit default in the persisted
schema. `required` means that a missing, blocked, inactive, or incompatible
component blocks the plugin for the affected target. `optional` permits a
visible omission without blocking the plugin. `recommended` is valid only for
instructions and always remains visibly inactive until the existing explicit
instruction activation flow from #468 succeeds.

An agent member may use the long-form `fallback` table shown above. Version 1
accepts only `kind = "inline"` with one skill reference. That skill must also be
a `required` member of the same plugin, so fallback closure is explicit and
cannot pull in behavior through a hidden second path. The fallback identifies
authored canonical behavior for targets without an isolated subagent. It does
not change the referenced `AGENT.md`, does not invent prompt text, and does not
make the fallback skill optional. A valid fallback may satisfy a required
behavioral capability, but never a required isolation, permission, or other
safety boundary. A fallback on any other member kind, an unknown fallback kind,
or a fallback that widens a portable safety boundary is invalid.

Plugin-local `[[tool]]` and `[[hook]]` arrays are reserved, independently
versioned extension points for the active-code slices. Every descriptor carries
its own integer `schema_version`; #499 owns tool descriptor version 1 and #500
owns hook descriptor version 1. This lets the passive `PLUGIN.toml` schema stay
at version 1 without silently changing the meaning of an already accepted
descriptor. An implementation that does not support the exact descriptor kind
and version reports `unsupported_active_component_schema` and cannot partially
activate the package as a passive plugin. Unknown descriptor versions are
blocking and are never interpreted as the closest known version.

A `[[tool]]` or `[[hook]]` declaration itself establishes membership;
`plugin.tools`, `plugin.hooks`, or any second membership list is invalid. Local
IDs use the plugin-name grammar and are unique per component kind inside the
plugin. Paths resolve from the directory containing `PLUGIN.toml` and must stay
inside that package after lexical and filesystem containment checks.

When those schemas land, the tool owns the event-independent named-input schema
and the only canonical exec-style argument template. A hook may bind typed
event fields to named inputs but cannot append, replace, or reinterpret the
argument vector. Tool approval covers the invocation envelope; hook approval
covers the binding and referenced tool invocation-contract hash. Runtime values
are validated inputs, not approval identities.

### Reference and component identity grammar

Source IDs, slot names, and stable IDs cannot contain `:` or `#`. Portable
component references therefore have an unambiguous grammar:

```text
<kind>:<selector>                    # declaring source only
<kind>:<source-id>:<selector>        # exact cross-source reference
```

`<kind>` is `plugin`, `skill`, `agent`, or `instruction`. `<selector>` matches
first by exact slot name and then by exact stable ID where that component type
supports one. If those two lookups identify different candidates, the reference
is ambiguous and blocking. References never fall back to a same-named asset in
another source.

Examples:

```text
skill:impeccable                     # same source
agent:design-platform:reviewer       # exact cross-source source and agent
plugin:company-engineering           # same-source plugin dependency
plugin:platform:company-engineering  # exact cross-source plugin dependency
```

The canonical identities are:

```text
plugin                 <source-id>:<plugin-name>
plugin-local tool      <source-id>:<plugin-name>#tool:<local-id>
plugin-local hook      <source-id>:<plugin-name>#hook:<local-id>
```

The source-qualified plugin identity remains stable for the selected slot.
`plugin.id` is retained separately for move detection and selector matching; it
never replaces the visible source-qualified identity silently.

### Authored-stack selection grammar

The root `dalo.toml` remains the authored-stack and source-composition manifest.
It references plugins without embedding their definitions:

```toml
schema_version = 1

[source]
id = "design-platform"
name = "Design Platform"
kind = "team"

[selection]
plugins = [
  { ref = "design-platform:impeccable", requirement = "required" },
  { ref = "design-platform:design-docs", requirement = "recommended" },
]
```

Every stack selection is source-qualified as `<source-id>:<selector>`. The
declaring manifest's `[source].id`, when present, must match its configured
source ID before its selections are accepted. `[source].id` is required when a
manifest contains `[selection]`. This makes self-references stable without a
magic `self` alias and keeps cross-source references auditable.
Catalog-derived source IDs use the existing deterministic team/catalog
namespacing before matching.

`requirement` is required and is either `required` or `recommended`:

- `required` contributes selected intent whose absence or policy decline makes
  the authored stack incomplete;
- `recommended` contributes selected intent that may be explicitly declined or
  degraded without making the entire stack invalid, but the decline remains
  visible.

An identical selection repeated in one manifest is invalid. If independent
stack manifests select the same qualified plugin, Dalo retains every stack
origin and uses the strongest requirement (`required` over `recommended`). It
does not let source traversal order choose the result.

`dalo plugin select <source-id>:<selector>` records an additive direct-user
origin with required intent in local user configuration. `plugin unselect`
removes only that direct origin. It does not edit a source manifest, remove a
stack or dependency origin, or create an implicit decline. A separate explicit
policy operation is required to decline stack-selected intent.

The first implementation bumps user configuration to version 2 and persists
that local intent separately from sources:

```toml
version = 2

[plugins]
direct = ["design-platform:impeccable"]

[[plugin_policy]]
layer = "user_local"
rule_id = "decline-design-docs"
plugin = "design-platform:design-docs"
decision = "decline"
reason = "Not used on this workstation"
```

`plugins.direct` is a sorted set of canonical source-qualified plugin
references. Version 2 initially accepts only the `decline` user-policy decision;
fallback selection is added only when a concrete target policy schema exists.
`layer` must be `user_local`; unknown layers are rejected. `rule_id` is unique
in user configuration and uses the plugin-name grammar. `reason` is required
non-empty audit context but does not participate in winner selection. CLI
commands own these mutations so `unselect` and `decline` cannot be confused
accidentally.

### Selection closure and policy precedence

Selection is a deterministic graph computation, not last-writer-wins:

1. normalize all valid authored-stack and direct-user references to canonical
   plugin identities;
2. union their origins and retain provenance for every origin;
3. choose provisional plugin winners by slot using
   `(source priority asc, source ID asc)`;
4. expand only provisional winners' dependencies, then recompute winners and
   reachability from the original stack/direct roots;
5. repeat step 4 until the reachable identity set and winners are stable;
6. if a previous graph state repeats, report `dependency_winner_cycle` and
   block the involved slots instead of choosing by iteration order;
7. union dependency origins from the stable reachable graph and keep the
   strongest requirement;
8. apply policy decisions without deleting selection origins;
9. resolve members in their own component namespaces.

Only selected plugin candidates participate in plugin slot conflicts. A
non-selected same-name package cannot shadow selected intent. Two selected
same-slot plugins follow normal deterministic winner selection and the loser is
reported with its still-visible origins.

Selection origins are exactly `stack`, `direct`, and `dependency`. Each origin
record includes its declaring source or local configuration, manifest/plugin
identity, source revision when available, and requirement. A policy decision is
stored separately with `layer`, `rule_id`, provenance, decision, reason, and
optional recorded-at metadata. It is never encoded as a fourth origin.

Version 1 deliberately has no plugin-target-policy or authored-stack-policy
schema. Its complete precedence is limited to mechanisms that have an on-disk
grammar:

1. portable safety requirements are non-overridable;
2. a component's declared `required`, `optional`, or `recommended` membership
   and its optional inline fallback determine component coherence;
3. a stack selection's `required` or `recommended` strength determines whether
   a visible decline makes the wider stack incomplete;
4. a user-local `decline` may suppress activation but cannot erase intent,
   select a different fallback, or weaken steps 1–3.

For version 1, a persisted policy decision therefore has
`layer = "user_local"` and `decision = "decline"`. Component fallback is
authored membership data, and stack selection strength is origin data; neither
is serialized as a policy decision. Provider overlays are adapter inputs, not a
policy layer. Plugin-target policy, stack-target policy, or user fallback
selection requires a future RFC amendment with an exact schema and total
precedence before it can affect resolution.

No policy may invent undeclared executable behavior, turn an inactive
instruction recommendation into activation, relabel an unsupported mapping as
safe, or weaken a portable safety boundary. `blocked` dominates every declared
fallback.

An explicit decline preserves the selected identity and all origins with a
`declined` policy result. Declining required stack or dependency intent makes
that stack/plugin incomplete; declining recommended intent is a visible
non-blocking omission. This is how local choice can override activation without
silently rewriting authored intent.

### Dependency and target coherence

`plugin.requires[]` accepts `required` or `optional`. Dependencies are expanded
only from a selected plugin winner. An unqualified dependency resolves only in
the declaring source. A cross-source dependency must name the exact source and
never selects a matching asset from an unrelated source. A missing or cyclic
required dependency is blocking. Cycles are reported as one deterministic
strongly connected component; they are never broken by traversal order.

Canonical plugin and member resolution is target-independent. Target adapters
consume that result only after winners, dependency closure, approvals, and
policy facts are fixed.

Coherence is atomic per `(canonical plugin, target)`:

- a blocked required member, dependency, safety capability, or required active
  instruction blocks all native writes owned by that plugin for that target;
- optional members may be omitted only with an explicit compatibility finding;
- recommended instructions remain inactive and do not block;
- already active independently managed assets are not removed merely because a
  plugin projection blocks;
- another target or unrelated plugin may still reconcile safely.

Adapters stage the complete owned projection before changing target state. A
failed or interrupted apply restores the previous projection for that
plugin-target pair and never leaves a silently partial native plugin.

### Activation and trust matrix

Selecting a plugin authorizes resolution of inert intent only. It grants no
content, agent, instruction, execution, hook, generator, source, author, or
organization approval.

| Component | Effect of selected membership | Additional activation boundary |
| --- | --- | --- |
| Skill | Desired resolver candidate | Existing skill approval, audit, and target materialization rules |
| Agent | Desired resolver candidate | RFC 0004 agent approval, audit, dependency, and projection rules |
| Instruction | Availability/requirement check only | Existing explicit `instructions enable` flow; never automatic |
| Tool | Visible pending active component | #499 component-specific audit and execution approval |
| Hook | Visible pending active component | Approved tool plus #500/#501 hook-scope approval |
| Generator | Visible delivery requirement | #494 exact recipe/tool/revision approval |

A source-level content approval may continue to match skills according to the
existing approval schema, but it does not become plugin, agent, tool, or hook
approval. There is no universal plugin approval record. Revocation changes only
the matching component state, then recomputes plugin-target coherence.

### Bounded parsing, hashing, and diagnostics

The package parser is bounded and fail-closed:

- `PLUGIN.toml` is valid UTF-8 and at most 1 MiB;
- strings are at most 16 KiB, lists at most 1,024 entries, and nested TOML
  tables at most 32 levels;
- one package contains at most 4,096 filesystem entries, at most 32 directory
  levels, at most 64 MiB per regular file, and at most 256 MiB total regular
  file content;
- duplicate TOML keys, duplicate local component IDs, duplicate member
  declarations, unknown portable keys, absolute paths, parent traversal,
  symlinks, sockets, devices, and escaping paths are blocking;
- unknown keys are allowed only below `providers.<provider>`, remain inert, and
  are retained for the owning adapter to validate.

Bounds are checked before allocation or full reads where possible. Exceeding a
bound produces a typed malformed-package diagnostic; content is never truncated
and accepted. One malformed plugin remains visible but does not erase valid
siblings from the source inventory.

Three hash boundaries remain distinct:

1. `package_hash` covers every regular file below the plugin directory in
   lexicographic relative-path order. Each entry contributes path bytes, file
   kind, executable bit, byte length, and exact bytes. It includes
   `PLUGIN.toml`, native overlays, and support files, but not referenced skills,
   agents, or instructions outside the package.
2. `closure_hash` covers the canonical plugin identity, package hash, reachable
   dependency identities, resolved member identities and content hashes,
   requirements, effective inclusion or decline outcomes, and the selected
   fallback identity when one changes the effective closure. It excludes why an
   identical result was selected: origins, policy layer, rule ID, provenance,
   reason, and timestamps do not participate. Linked targets and adapter state
   are also excluded.
3. component approval hashes cover only the security-relevant closure defined
   by that component RFC. In particular, a tool approval does not churn because
   an unrelated plugin README or member changes.

Source ID, source kind, immutable commit when available, package relative path,
slot name, stable ID, and both canonical hashes are retained as provenance.
Line endings and TOML formatting are not normalized before package hashing.
Selection origins and policy provenance are compared structurally in the user
lock and produce their own drift findings; adding an equivalent second origin
or renaming a policy rule cannot invalidate a native projection. A target
projection fingerprint derives from `closure_hash`, target ID, adapter version,
and relevant native inputs rather than from provenance metadata.

Unknown `schema_version` values and unknown portable fields block that package.
Dalo does not guess, downgrade, or rewrite source manifests. Additive native
overlay data may be preserved only through an adapter that understands its
ownership rules. Persisted local config, lock, and target-state schema changes
use explicit version bumps and reviewed migrations; source content is never
silently migrated in place.

### Canonical lock and target-state ownership

The persisted boundary is field-level:

| Store | Owned facts |
| --- | --- |
| Root `dalo.toml` | Authored stack selections |
| User config | Direct selections and explicit user-local policy decisions |
| `source-lock.toml` | Catalog pins plus plugin inventory snapshots used for drift comparison |
| `approvals.toml` and audits | Existing per-asset and future component-specific trust records |
| User `lock.toml` | Canonical plugin identity/provenance, package and closure hashes, all selection origins, dependency closure, resolved members, requirements, policy-decision provenance, approval state, and canonical blocked reasons |
| Internal target state | Target ID, adapter and verification version, compatibility findings, chosen fallback, owned paths, staged/native fingerprints, apply status, and rollback metadata |

The user lock contains no native path, provider version, or target-specific
winner. Target state cannot introduce a different canonical component winner or
erase an origin recorded in the user lock. `status`, `doctor`, `plan`, dry-run,
and JSON join both stores without pretending target drift changed authored
intent.

### Invalid examples

All of these fail closed:

```toml
# Directory is plugins/impeccable but the slot name differs.
[plugin]
name = "impeccable-codex"
```

```toml
# Root stack selections must be source-qualified and explicit about strength.
[selection]
plugins = [{ ref = "impeccable" }]
```

```toml
# A duplicate member list cannot grant tool membership a second way.
[plugin]
tools = ["detector"]

[[tool]]
schema_version = 1
id = "detector"
```

```toml
# Hooks bind named inputs; they never own another argv surface.
[[hook]]
schema_version = 1
id = "after-ui-write"
tool = "detector"
argv = ["--path", "${event.path}"]
```

A dependency `plugin:company-engineering` in source `design-platform` means
`design-platform:company-engineering`; it cannot be satisfied by
`public:company-engineering`. The latter must be written exactly as
`plugin:public:company-engineering`.

### First-slice acceptance matrix

| Fixture | Canonical result | Target result | Mutation rule |
| --- | --- | --- | --- |
| Passive plugin with one approved skill and agent | One selected plugin with two resolved required members | `exact` or verified `mapped` per adapter | Stage and apply one coherent plugin-target projection |
| Required plugin dependency chain A → B → C | Fixed-point closure with three identities and dependency origins | Each target plans from the same canonical closure | Never select by traversal order or unrelated slot match |
| Optional agent unsupported on Cursor | Plugin remains selected; agent omission is recorded | `unsupported`, plugin degraded but not blocked | Apply remaining coherent optional projection |
| Required blocking hook mapping on one target | Canonical plugin remains selected | Affected plugin-target pair is `blocked` | Write nothing for that pair; other targets may proceed |
| Recommended instruction pack not enabled | Recommendation and inactive state remain visible | Non-blocking `inactive` finding | Never write an instruction block implicitly |

### Target overlays

Portable declarations are the default. Native overlays may select or strengthen
a native representation. They may not weaken portable safety boundaries, alter
canonical identity, create a provider-specific winner, or spread provider
conditionals through canonical skill and agent bodies. Unknown overlay values
remain inert until the owning adapter can validate and safely re-emit them.

## Component model

### Skills

Agent Skills remain canonical skill packages. Direct linking or copying remains
the default materialization strategy.

Provider-specific skill derivations, prebuilt mappings, and trusted generators
belong to the delivery model in #494. A plugin may reference the resulting
logical skill without making generated output the authored source of truth.

### Instruction packs

Instruction packs remain managed Markdown blocks with the existing strict
ownership boundary. Plugin membership can enable discovery and recommend
activation, but installation must not silently inject standing global guidance.

Issue #468 should define source-backed activation and target-aware fan-out.
This RFC composes those enabled packs into the effective configuration rather than
replacing their activation model.

### Agents

Agents use the canonical `AGENT.md` packages and adapters from RFC 0004.
Plugin membership declares that a role belongs to the product capability; it
does not bypass agent-specific approval, audit, dependency, or compatibility
checks.

When a target cannot provide an isolated native subagent, the first plugin
slice may declare an `inline` execution fallback on that plugin's agent
membership. This is composition policy, not an implicit field added to RFC
0004's `AGENT.md` schema. The membership must reference canonical authored
fallback behavior and Dalo must not invent behavioral text during projection.
Version 1 permits only the required-skill inline fallback and precedence
defined in the canonical contract above. Broader target-policy fallbacks
require a later RFC amendment. A reusable fallback intrinsic to every use of an
agent would require a focused RFC 0004 amendment.

### Local tools

A local tool is an executable entry point declared inside a reviewed portable
plugin package. Its descriptor has an independently versioned schema owned by
#499. Its local ID is unique inside that plugin and its canonical identity is
derived from the source-qualified plugin identity, for example
`company:impeccable#tool:detector`. The first vocabulary should be intentionally
narrow:

- runtime;
- plugin-root-relative entry path;
- event-independent named inputs and one canonical exec-style argument
  template;
- declared portable capabilities;
- platform requirements;
- content hash and provenance.

Tools are active code. Merely approving a skill or plugin must not authorize
tool execution. Dalo may install or reference a tool only after the applicable
code-execution approval exists. The tool approval is scoped to a deterministic
component closure: source identity and qualified tool identity, descriptor,
entry path and referenced executable bytes, runtime policy, named-input schema,
argument template, working-directory/environment policy, and capabilities.
The containing source revision and whole plugin-package hash remain audit and
provenance facts; unrelated plugin changes must not invalidate an unchanged
tool closure.

Provider adapters should prefer stable plugin-root substitutions over
hard-coded harness skill paths. Generated commands must resolve inside the
owned projection or immutable source tree and must never interpolate untrusted
event data through a shell string.

### Hooks

Hooks are independently versioned plugin-local descriptors in the first
active-code slice; #500 owns their descriptor schema. A hook has a local ID,
receives a source-qualified identity such as
`company:impeccable#hook:after-ui-write`, and references an approved tool from
the same plugin by local logical ID. Cross-plugin executable references are
reserved until concrete reuse cases justify a separate publication model.

The normative portable hook vocabulary belongs to #500 rather than this parent
RFC. It must model semantic subject, phase, requested effect, typed input and
output, timeout, failure policy, and composition behavior. #500 publishes typed
event payload fields; #501 validates hook bindings against the generic tool
input contract from #499 and derives the invariant effective invocation shape.

At minimum, the vocabulary must keep these concepts distinct:

```text
session.end                  # observational final lifecycle event
workflow.completion-attempt  # may request that the agent continue
```

Provider event names such as `Stop` are adapter inputs, not portable semantic
names. A target's completion-attempt hook must never be presented as final
session termination, and an advisory session-end hook cannot satisfy required
completion enforcement.

A file event may be derived from a tool event only when the adapter can
deterministically identify the affected path and operation. Otherwise the
mapping is degraded or unsupported, never assumed exact. Hook outputs,
blocking behavior, timeouts, and user-visible messages remain part of the
adapter contract.

### MCP servers

MCP distribution is reserved in the general model because provider plugins
commonly include it, but it should not be in the first implementation slice.
It adds transport, process, network, authentication, secrets, and tool-approval
boundaries beyond local hook execution.

## Compatibility and degradation

Reuse RFC 0004's ordered field-level outcomes:

```text
exact < mapped < guidance_only < unsupported < blocked
```

- `exact`: the native projection preserves the semantics directly;
- `mapped`: a verified native representation preserves the contract;
- `guidance_only`: intent survives only as model instructions or an explicit
  manual step;
- `unsupported`: an optional behavior cannot be projected;
- `blocked`: projection would be unsafe, invalid, or violate a required
  boundary.

Examples:

| Capability | Target result |
| --- | --- |
| Standard `SKILL.md` | `exact` through direct materialization |
| Claude Markdown agent to canonical agent to Codex TOML | `mapped` when RFC 0004 fields are enforceable |
| Fresh reviewer role with no target subagent support and declared inline fallback | `guidance_only` |
| `file.after-write` implemented as verified `tool.after` filtering | `mapped` or `guidance_only`, depending on event fidelity |
| Write-blocking pre-hook mapped to a post-write notification | `blocked` when blocking is required |
| Optional command-palette hint on a target without that UI | `unsupported` |

Every projection report should include:

- component and field;
- requested portable intent;
- target capability;
- compatibility result;
- native artifact or fallback;
- remediation when blocked.

No unsupported component should disappear silently from `sync`, `status`,
`doctor`, dry-run, or JSON output.

## Materialization and ownership

The existing direct-symlink path remains the default for portable skills.
Richer projections introduce typed owned artifacts, not permission to write
arbitrary provider configuration.

Adapters may own only paths declared by their contract, for example:

- a Dalo-owned plugin directory;
- a Dalo-owned agent file;
- a Dalo-owned skill link;
- a marked instruction block;
- a dedicated hook manifest;
- a known merge-safe section in a provider settings file.

If a provider requires merging into a user-owned file, the adapter must define:

- ownership markers or a structural ownership key;
- compare-and-swap or equivalent concurrent-edit protection;
- exact rollback behavior;
- malformed-owned-section recovery;
- foreign-content preservation;
- disable and uninstall semantics.

Source checkouts remain immutable inputs. Generated or normalized projections
live in Dalo-owned staging and are promoted atomically after audit.

## Trust and security model

Plugin approval is not a universal capability grant. Trust remains
component-specific:

| Component | Minimum trust boundary |
| --- | --- |
| Skill/instruction content | Content approval and audit |
| Agent | Agent activation approval and audit |
| Local tool | Exact code/provenance execution approval |
| Hook | Tool approval plus event, timing, and blocking-scope approval |
| Generator | Exact recipe/tool/revision approval from #494 |
| MCP server | Separate process/network/auth/tool approval in a later RFC |

Existing Agent Skill packages may contain `scripts/`, but those files remain
inert content from Dalo's perspective: Dalo neither registers nor invokes them,
and any later execution is mediated by the agent harness and its user-facing
permission model. A Dalo-managed tool or hook is different because Dalo
publishes an executable contract or automatic registration. That integration
must never point at a mutable direct skill path and therefore needs immutable
bytes plus Dalo-owned component approval. This distinction preserves current
direct-linked skills rather than deprecating their support scripts.

The safe interaction should still be coherent. #503 adds an aggregated
`dalo plugin review <ref>` session that presents every pending decision and can
record the explicitly reviewed set together, while preserving separate content,
agent, tool, and hook approval identities. It must not create a blanket plugin,
source, author, or organization execution grant.

Required properties:

- newly discovered active code never runs during non-interactive sync;
- changing a security-relevant member of a tool or hook's component-specific
  contract closure invalidates only the applicable approval;
- dry-run never executes tools, generators, hooks, or servers;
- staging has bounded filesystem access and cannot write directly to targets;
- target projections are audited before activation;
- an unsupported optional feature may degrade only when an authored fallback
  permits it;
- a required safety or enforcement capability fails closed;
- revocation removes or disables the affected owned projections without
  disturbing unrelated components.

## Resolver, stack, and installation model

Assets continue to resolve in their own namespaces. A portable plugin does not
replace skill, instruction, or agent winner selection. It contributes
dependency and activation relationships after canonical winners are known.

An authored stack contributes the wider composition: selected sources and
plugins, standalone assets, shared instructions, routing, and policy. The first
slice can use the existing source-composition repository and root `dalo.toml`
as that boundary without inventing another manifest format.

Conceptually:

```text
source inventories + authored stack selections + direct user selections
      |
      v
asset resolution by namespace + portable-plugin dependency closure
      |
      v
approval + audit state
      |
      v
canonical target-independent resolution

canonical resolution + linked targets + adapter capabilities + target state
      |
      v
effective installation plan
      |
      v
target projections + owned materialization operations
```

Linked targets and adapter capabilities may change compatibility findings and
native operations. They must never change canonical plugin, skill, instruction,
or agent winner selection.

The user lock records the canonical identities, origins, hashes, dependency and
member closure, policy provenance, approval state, and canonical blockers
defined above. Internal target state records adapter versions, compatibility,
fallbacks, owned paths, projection fingerprints, rollback facts, and
target-specific omissions. Neither store may absorb the other's authority.

The resolved result remains inspectable state, not a second kind of authored
stack.

## CLI experience

Illustrative commands:

```sh
dalo plugin list
dalo plugin show company:impeccable
dalo plugin select company:impeccable
dalo plan --target codex
dalo plan --target claude --json
dalo approve tool company:impeccable#tool:detector
dalo approve hook company:impeccable#hook:after-ui-write
dalo sync --dry-run
```

`dalo plugin select` adds direct user intent; it does not mutate a source-authored stack selection. `dalo plan` is the key UX. Before any mutation, it should answer:

```text
Plugin: company:impeccable

codex
  skill impeccable                  exact       direct Agent Skill
  agent finish-reviewer             mapped      Codex TOML
  hook after-ui-write               mapped      PostToolUse filter
  hook stop-review                  exact       Stop
  instruction engineering-defaults  inactive    explicit enable required

cursor
  skill impeccable                  exact       direct Agent Skill
  agent finish-reviewer             mapped      readonly Cursor agent
  hook after-ui-write               blocked     required post-write event unavailable
  hook stop-review                  unsupported optional
```

The plan should distinguish:

- unavailable target capability;
- missing approval;
- malformed source;
- unsupported optional behavior;
- required blocked behavior;
- user-declined native integration;
- portable fallback selected.

## Adapter contract

RFC 0004's adapter contract should be generalized rather than replaced. Each
adapter publishes:

- discovery and owned materialization paths;
- supported component types;
- verified event and permission mappings;
- native syntax and version baseline;
- compatibility classification functions;
- merge/ownership strategy for sidecars;
- trust and enablement behavior;
- uninstall and rollback behavior;
- acceptance fixtures or snapshots.

Adapters must not infer enforcement from prompt text. A provider version change
that invalidates a relied-on mapping requires an adapter update before Dalo
continues claiming `exact` or `mapped`.

Native overlays are retained and re-emitted only through the owning adapter.
Unknown native values never become portable by accident.

## Proposed implementation slices

### Slice 0: RFC and terminology

- Freeze component, portable-plugin, authored-stack, projection, and adapter boundaries.
- Specify how the bounded `PLUGIN.toml` package composes with root `dalo.toml`.
- Specify identity, selection, policy, hashing, activation, and state ownership.
- Document how this RFC composes RFC 0004, #468, and #494.

### Slice 1: portable plugins with existing passive assets

- Discover plugins that reference existing skills, instruction packs, and
  agents.
- Read authored-stack selections from root `dalo.toml` manifests and direct
  user selections while preserving their origins.
- Resolve dependency closure without new tool or hook execution.
- Add `plugin list/show/select/unselect` and read-only `dalo plan`.
- Ensure `plugin unselect` removes only direct user intent and cannot rewrite
  or silently suppress authored-stack selection.
- Record plugin provenance and selection origins in locks and JSON output.

### Slice 2: portable local tools

- Add bounded tool manifests, component-specific hashing, auditing, and
  separate execution trust.
- Define event-independent named inputs and canonical exec-style argument
  templates.
- Reference tools through stable logical IDs and immutable plugin-owned roots.
- Do not install hooks yet.

### Slice 3: hook model and two verified adapters

- Define the first semantic hook events.
- Implement Claude and Codex hook projections.
- Add explicit hook approval, owned sidecars, rollback, `status`, and `doctor`.
- Bind verified event fields to named tool inputs without a second argument
  template, including Windows launcher/quoting fixtures.
- Add #503's aggregated review flow without merging approval identities.
- Test exact, mapped, unsupported, and blocked cases.

### Slice 4: native plugin projection

- Render a selected passive plugin as a Codex plugin and at least one other
  native provider plugin after #497/#498.
- Keep tool/hook components visible as pending or blocked until #499/#501 are
  available; active-code integration retains those dependencies.
- Keep ordinary skill-only installs on the direct path.
- Integrate target-aware derivation from #494 without making generator
  execution implicit.

### Later slices

- Cursor and OpenCode adapters;
- MCP server declarations;
- first-class named or repository-specific stack manifests beyond
  source-level stacks;
- provider marketplace metadata;
- richer stack composition and routing.

## Acceptance criteria for this RFC

- [ ] Terminology distinguishes an individual Agent Skill, RFC 0004 Agent
      Package, portable plugin, authored agent stack, native provider plugin,
      and effective target state.
- [ ] The canonical model reuses standard Agent Skills unchanged.
- [ ] Existing instruction-pack and portable-agent semantics remain intact.
- [ ] The manifest can group skills, agents, tools, and hooks without embedding
      provider paths in portable fields.
- [ ] Hook and tool declarations have explicit trust and provenance boundaries,
      deterministic invocation composition, and component-specific approval
      closures that ignore unrelated plugin-package changes.
- [ ] Unqualified dependencies resolve same-source only; cross-source references
      are explicit and cannot be satisfied by unrelated slot-name matches.
- [ ] Compatibility outcomes reuse or deliberately amend RFC 0004's
      field-level model.
- [ ] Required capabilities fail closed; optional behavior degrades only through
      an authored fallback.
- [ ] Target adapters own bounded artifacts and define safe merge, rollback,
      and uninstall semantics.
- [ ] The resolver can explain one logical plugin across at least Codex and
      Claude projections.
- [ ] Existing direct skill symlinking and inert support scripts remain backward
      compatible and the default.
- [ ] One aggregated review session can preserve all component-specific approval
      identities as defined by #503.
- [ ] The design composes with #468 and #494 instead of duplicating them.
- [ ] Implementation is split into passive composition, active code, hooks, and
      plugin projection rather than landing one broad trust expansion.

## Open questions

1. What is the smallest semantic hook vocabulary that Codex and Claude can
   both support without overstating equivalence?
2. Which provider version and acceptance fixtures are required before an
   adapter can claim hook enforcement?
3. Is MCP configuration part of the portable-plugin manifest's eventual stable
   schema, or a separate RFC that only references portable-plugin IDs?
4. Which concrete reuse case would justify standalone executable packages,
   cross-plugin tool/hook references, or an additional named `STACK` manifest?
5. Which concrete workflow would justify adding plugin-target, stack-target, or
   user fallback-selection policy beyond version 1's explicit local decline?

## References

- [Review of RFC #495 and sub-issues #496–#502](https://github.com/sebastian-software/dalo/issues/495#issuecomment-5135222277)
- [Aggregated plugin review flow #503](https://github.com/sebastian-software/dalo/issues/503)
- [Agent Skills specification](https://agentskills.io/specification)
- [Dalo RFC 0004: Portable Agent Profiles](https://github.com/sebastian-software/dalo/blob/main/docs/rfcs/0004-portable-agent-profiles.md)
- [Codex plugin packaging](https://developers.openai.com/plugins/build/plugins)
- [Impeccable](https://github.com/pbakaus/impeccable)
- #468 — source-backed instruction packs and target-aware fan-out
- #494 — provider-specific skill derivations

---

Drafted with assistance from OpenAI Codex.

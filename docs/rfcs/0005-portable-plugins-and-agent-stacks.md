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

## Proposed source model

A portable plugin uses a bounded package such as
`plugins/impeccable/PLUGIN.toml`, while the root `dalo.toml` remains focused
on authored stack and source composition. Issue #496 owns the exact grammar,
identity, and migration contract. The illustrative shape below is not yet a
committed schema:

```toml
schema_version = 1

[plugin]
id = "impeccable"
name = "Impeccable"
description = "Frontend design, review, and quality workflows."
skills = ["impeccable"]
agents = [
  "impeccable-finish-reviewer",
  "impeccable-documenter",
  "impeccable-asset-producer",
]

[[tool]]
id = "detector"
runtime = "node"
entry = "./tools/detector/detect.mjs"
capabilities = ["read-files"]
inputs = { changed_file = "path" }
argv = ["--changed-file", "${input.changed_file}"]

[[hook]]
id = "after-ui-write"
event = "file.after-write"
tool = "detector"
bindings = { changed_file = "${event.path}" }

[hook.filter]
paths = ["**/*.{html,css,scss,jsx,tsx,vue,svelte}"]

[hook.fallback]
kind = "workflow-completion-attempt"
required = false
```

Skills, instruction packs, and agents remain canonical assets referenced by
ID; their prompts, frontmatter, and permission declarations are not duplicated.
Tool and hook descriptors are plugin-local in the first active-code slice and
receive derived, source-qualified identities plus separate approvals. Declaring
one `[[tool]]` or `[[hook]]` descriptor establishes that component's plugin
membership; a second top-level membership list is neither needed nor allowed.
Descriptor and entry paths resolve from the directory containing `PLUGIN.toml`.

The tool owns an event-independent named-input schema and the only canonical
exec-style argument template. A hook may bind typed event fields to those named
inputs, but it cannot append, replace, or reinterpret the tool's argument
vector. Tool approval covers the permitted invocation envelope; hook approval
covers the binding plus the referenced tool invocation-contract hash. Runtime
values are validated against that contract but are not themselves approval
identities.

### Stack composition

The root `dalo.toml` may select portable plugins by qualified reference without
embedding their definitions:

```toml
[selection]
plugins = ["design-platform:impeccable"]
```

The exact syntax and required/recommended semantics belong to #496. Authored
stack selection is reproducible source intent. `dalo plugin select` records a
separate direct user selection overlay; it does not edit or erase the authored
stack.

### Plugin dependencies

A plugin may require other plugins or individual assets:

```toml
[plugin]
id = "frontend-quality"
skills = ["frontend-review"]
requires = [
  "plugin:company-engineering",
  "skill:accessibility-audit",
]
```

Dependencies participate in deterministic closure, approval, lock, audit, and
provenance handling. Unqualified typed references resolve only within the
declaring plugin's source. Cross-source dependencies must use the exact
source-qualified reference grammar selected by #496; they are never satisfied
by matching an unrelated source's slot name. Dependencies do not silently
approve active code from another source.

### Target overlays

Portable declarations are the default. Native overlays are an explicit escape
hatch:

```toml
[plugin.providers.codex]
interface = "./providers/codex/interface.toml"

[hook.providers.claude]
timeout_seconds = 30
```

An overlay may select or strengthen a native representation. It may not weaken
portable safety boundaries. Provider conditionals should not spread through
portable skill bodies merely because an overlay exists.

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
fallback behavior, such as a required skill or bounded prompt asset; Dalo must
not invent behavioral text during projection. #496 owns the long-form
membership grammar and total fallback precedence. A reusable fallback intrinsic
to every use of an agent would require a focused RFC 0004 amendment.

### Local tools

A local tool is an executable entry point declared inside a reviewed portable
plugin package. Its local ID is unique inside that plugin and its canonical
identity is derived from the source-qualified plugin identity, for example
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

Hooks are plugin-local descriptors in the first active-code slice. A hook has a
local ID, receives a source-qualified identity such as
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

The lock and target state should record:

- authored stack/source identity and revisions where applicable;
- selected portable-plugin identities and source revisions;
- canonical component hashes;
- dependency closure;
- target adapter and verification version;
- compatibility summary;
- active fallback choices;
- native projection fingerprints;
- approval provenance;
- blocked or intentionally omitted components.

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

- Agree on component, portable-plugin, authored-stack, projection, and adapter boundaries.
- Decide how a bounded portable-plugin manifest composes with the stack-level root `dalo.toml`.
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

1. What exact root-`dalo.toml` grammar expresses required or recommended
   portable-plugin stack selections?
2. What explicit local policy can decline or suppress a stack-selected plugin
   without rewriting authored intent or hiding the omission?
3. What is the complete inventory hash and migration contract for one bounded
   `PLUGIN.toml` package, distinct from each security-relevant component
   approval closure?
4. What is the smallest semantic hook vocabulary that Codex and Claude can
   both support without overstating equivalence?
5. Should fallback behavior live on each component, on a plugin or stack target
   policy, or at several layers with a total precedence rule?
6. How should a plugin recommend an instruction pack without silently enabling
   always-loaded guidance?
7. Which provider version and acceptance fixtures are required before an
   adapter can claim hook enforcement?
8. Which canonical facts belong in the user lock versus target-specific
   projection state?
9. Is MCP configuration part of the portable-plugin manifest's eventual stable
   schema, or a separate RFC that only references portable-plugin IDs?
10. Which concrete reuse case would justify standalone executable packages,
    cross-plugin tool/hook references, or an additional named `STACK` manifest?

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

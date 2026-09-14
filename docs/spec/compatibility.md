# Portable Agent Packages — compatibility

This page records the evidence behind the draft 0.1 provider mappings. It is
an evidence matrix for the Dalo reference implementation, not a promise that
all agents interpret the same Markdown, event payload, or effect in the same
way.

The pinned adapter baselines are Claude Code **2.1.233** and Codex
**0.147.0**. A later provider version needs fresh evidence before a consumer
should claim compatibility. The draft adds no hook events, runtime downloads,
or MCP integration promises.

## Evidence matrix

| Provider or fixture | Supported package behavior | Evidence | Boundary and status |
| --- | --- | --- | --- |
| Claude Code 2.1.233 | `session/end` observation; `user_prompt/before` context or enforcement; tool-call before and after mappings; completion control | [Pinned adapter contract and upstream hook tests](../../tests/upstream_hooks.rs); Impeccable recorded/context coverage, plus an optional [real engine test](../../tests/upstream_hooks.rs) | Conditional on the named event, effect, and native payload adapter. A recorded run proves the fixture path, not every Claude host configuration. |
| Codex 0.147.0 | `session/end` observation; `user_prompt/before` context or enforcement; tool-call before and after mappings; completion control | [Pinned adapter contract and dispatch tests](../../tests/upstream_hooks.rs) using the Codex baseline probe | Conditional on the named event, effect, and native payload adapter. No claim of equivalent prompt, tool, or permission semantics with Claude. |
| Impeccable fixture | Context and recorded hook behavior; optional real engine execution when explicitly enabled | [Recorded-output tests](../../tests/upstream_hooks.rs) and the [upstream fixture guide](../../tests/fixtures/upstream-hooks/README.md) | The recorded/context path is the default evidence. Engine availability and output quality are separate from package validation. |
| Get Shit Done fixture | Live Node prompt guard on a Claude `PreToolUse` Bash payload | [Original guard test](../../tests/upstream_hooks.rs) invokes `gsd-prompt-guard.js` against suspicious and clean planning text | Covers the named guard and handler inputs only; it does not certify GSD's installer, other hooks, Codex `apply_patch` payloads, or other versions. |
| Planning with Files fixture | Live Cursor Bash reminder adapted to portable context and checked through Dalo's Codex output path | [Original reminder test](../../tests/upstream_hooks.rs) invokes the handler with and without `task_plan.md` | Covers the reminder adapter and supplied project context; it does not certify Cursor's hook protocol or the project's other wrappers. |
| Dalo author validator | Package contracts, local references and dependency cycles, plus separate provider hook capability reports | [Schema/parser conformance tests](../../tests/package_spec.rs), [CLI tests](../../tests/package_validate_cli.rs), and the [validator](../../src/package_validation.rs) | `dalo plugin validate` never executes tools or authorizes activation. External sources and runtime availability remain outside this source-only check. |
| Dalo package lifecycle | Selection, review, trust, staging, hook dispatch, update, revoke, removal, and user-file preservation | [Package lifecycle tests](../../tests/package_lifecycle.rs) exercise complete flows with fake provider probes and real Dalo/hook subprocesses | Both provider adapters are tested, including separately enabled project `AGENTS.md` and `CLAUDE.md` blocks. This is Dalo lifecycle evidence, not provider execution certification. |

## Effect matrix

The provider column describes the reference mapping at the pinned baseline. A
checkmark means the adapter has a bounded native projection; “conditional” also
requires the provider event and payload to match. A dash is unsupported for
that provider in this draft and must block a required hook.

| Portable effect | Claude Code 2.1.233 | Codex 0.147.0 | Evidence and limit |
| --- | --- | --- | --- |
| `observe` | ✓ conditional | ✓ conditional | Session-end observation requires user-only failure visibility. Claude completion observation also requires user-only failures to remain nonblocking. |
| `add_context` | ✓ conditional | ✓ conditional | [Impeccable recordings](../../tests/upstream_hooks.rs) compare the complete native context envelope. |
| `allow_deny` | ✓ conditional for supported pre-action events | ✓ conditional for supported pre-action events | Native permission decisions need event-specific payload adaptation; the Impeccable adapter rejects control output rather than downgrading it to advice. |
| `rewrite_input` | ✓ without changing the normal permission decision | ✓ with the required native `allow` pairing | Exact input shapes remain provider-specific; [dispatcher tests](../../src/hook_dispatch.rs) verify the distinct envelopes. |
| `replace_output` | ✓ on `PostToolUse` only | — | Claude's `PostToolUseFailure` has no replacement output. A required Codex hook using this effect is unsupported. |
| `continue_workflow` | ✓ conditional | ✓ conditional | Completion control is bounded to the named event; it does not promise equivalent stop behavior. |

Raw native event JSON is passed to handlers and is **not normalized by this
draft**. A portable handler that inspects it needs a provider-specific adapter
and payload tests. The [context adapter tests](../../tests/upstream_hooks.rs)
cover successful context translation and reject wrong-event or control-shaped
outputs; they do not certify every effect or native payload.

The [dispatcher tests](../../src/hook_dispatch.rs) cover failure policies,
audiences, incompatible output, and composition conflicts. User-only failures
produce native user diagnostics; model-and-user failures additionally use the
event's model channel. Session-end model delivery is unsupported on both
providers. On Claude, Stop feedback resumes the turn, so nonblocking failure
policies require user-only visibility. These limitations are checked before
projection and again at dispatch; unsupported required hooks cannot activate.
See the primary [Claude hook reference](https://code.claude.com/docs/en/hooks)
and [Codex hook reference](https://developers.openai.com/codex/hooks/) for native
channels. The test suite verifies Dalo's responses; it does not run complete
native model sessions.

## Reading the matrix

“Supported” means the reference implementation has a bounded mapping and
evidence for the named behavior. “Conditional” means the consumer still needs
the provider event, payload shape, runtime, permissions, and declared inputs
to line up. “Unsupported” means a required behavior must block activation; an
advisory fallback cannot silently replace it.

The portable result shapes and native response adaptations remain separate
contracts. In particular, a native `hookSpecificOutput` value is not portable
stdout, and a post-action effect cannot undo an action that already happened.
Provider payload details require provider-specific adapter tests; the package
schema does not normalize them.

See the [upstream integration guide](../../tests/fixtures/upstream-hooks/README.md)
for fixture commands, recordings, and the opt-in engine boundary. The
[versioned specification](README.md) defines the contract and the
[downloadable JSON Schema](plugin-v1.schema.json) covers structural fields;
the validator still owns containment, binding, and semantic checks.

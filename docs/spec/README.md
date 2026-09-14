# Portable Agent Packages — draft 0.1

Status: experimental author-facing profile, maintained in the Dalo repository.
This is a proposal for interoperability, not an adopted ecosystem standard.
It documents the existing `PLUGIN.toml` schema 1, tool descriptor 1, and hook
descriptor 1. The draft number does not change those persisted versions.

An author describes which skills, standing instructions, local tools, and
event handlers belong together. A consumer validates the description and
maps supported behavior to an agent harness. Package selection alone never
authorizes code execution or installation of standing instructions.

The specification is intended to be implementable without Dalo's CLI, store
layout, source-priority rules, approval ledger, or internal contract hashes.
Dalo is the first reference implementation. Its accepted architecture remains
in [ADR 0006](../adr/0006-passive-portable-plugins.md), with detailed semantics
and implementation history in [RFC 0005](../rfcs/0005-portable-plugins-and-agent-stacks.md).

## Start with a complete example

The [example source](../../examples/packages/source) contains:

```text
source/
  skills/design-review/SKILL.md
  instructions/design-context.md
  plugins/design-review/
    PLUGIN.toml
    context.mjs
```

The skill contains task-specific guidance. The instruction pack contains
standing guidance. The local Node tool emits a portable context result; the
hook binds the project directory to its named input before prompt submission.
The example is illustrative and is not an Impeccable integration.

Validate an author source without creating a store:

```sh
dalo --json plugin validate examples/packages/source
```

The validator prints report schema 1 with `valid: true` and exits 0 only when
at least one package is found, package contracts pass, and required references
resolve within the supplied source. Exit 1 means invalid or empty package
inventory or unresolved required references. Exit 2 means invalid command usage
or a missing source directory. Findings appear in `diagnostics`.

The command uses the production scanners and dependency rules. It reads
manifests and bounded package files without executing tools, downloading
dependencies, granting trust, or changing harness configuration. Package validity,
reference resolution, provider hook capabilities, and authorization are separate
report fields. Runtime availability and native handler behavior are not certified.

Use `--source-id company` for explicit same-source references such as
`skill:company:review`; the default source identity is `validation`. References
to other sources remain unresolved, since this command never consults the user's
source registry. See the [CLI reference](../reference.md#dalo-plugin-validate-source-path)
for exit codes and usage. From a checkout, replace `dalo` with
`cargo run --locked --`; the first Cargo build may fetch Rust dependencies.

[plugin-v1.schema.json](plugin-v1.schema.json) is a JSON Schema 2020-12 document
for editor assistance and structural validation of decoded TOML. Use a TOML-aware
editor or validate the equivalent JSON data model. Do not insert `$schema` as
a TOML property: unknown manifest fields are rejected. JSON Schema alone cannot
check file containment, required fallback closure, binding types, byte limits,
or provider semantics. The reference validator adds the local semantic checks.

## Package and reference contract

A source exposes packages only at `plugins/<name>/PLUGIN.toml`. Discovery is not
recursive. A manifest is UTF-8 TOML with these top-level fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Required integer, exactly `1`. |
| `plugin` | Required package metadata and passive membership. |
| `tool` | Optional array of local execution descriptors. |
| `hook` | Optional array of event-to-tool descriptors. |
| `providers` | Optional opaque provider overlays. Discovery acceptance does not authorize or certify their use. |

`plugin.name` must equal the directory name and use lowercase letters and digits
separated by single hyphens. `description` is required
and must contain non-whitespace text. Optional `id` uses lowercase alphanumeric
segments separated by dots or hyphens, up to 128 characters, and must be unique
within a source. Optional `version` is informational; it does not select releases.

`plugin.members` and `plugin.requires` are arrays. Every entry has an explicit
`ref` and `requirement`. A reference is `<kind>:<selector>` within the source or
`<kind>:<source>:<selector>` with an explicit source. Consumers supply the source
registry; the package cannot install or trust a source by naming it. Reference
atoms are nonempty strings of at most 16 KiB and cannot contain `:`, `#`, `/`,
or `\`. This is a reference grammar, not permission to use a selector as a
filesystem path. Consumers must validate resolved asset identities separately.

| Member kind | Requirements | Meaning |
| --- | --- | --- |
| `skill` | `required`, `optional` | References an existing Agent Skill. |
| `instruction` | `required`, `optional`, `recommended` | References a standing instruction pack. Activation is separate. |
| `agent` | `required`, `optional` | References an agent profile; this draft does not standardize a new profile format. |

`plugin.requires` accepts only `plugin` references with `required` or `optional`.
Duplicate references are invalid. Member order is not instruction precedence.
A required missing, inactive, blocked, or incompatible member blocks activation
for the affected target. Optional omission must remain visible. Recommended
instructions remain inactive until the consumer's user explicitly enables them.

Only an agent member may have `fallback = { kind = "inline", skill = "skill:…" }`.
That exact skill must also be a required member. An inline fallback may preserve
authored guidance; it must not be treated as equivalent to required agent
isolation or permissions.

Unknown fields and descriptor versions are invalid. Consumers must not silently
activate the known half of a package with an unsupported active descriptor.
The `providers` map is an explicit exception for inert adapter-owned data.

## Skills and standing instructions

Skills retain the [Agent Skills format](https://agentskills.io/specification).
Instructions remain Markdown, including content intended for
[AGENTS.md](https://agents.md/) or `CLAUDE.md`. A package can reference
`instruction:design-context`; it does not gain permission to overwrite either
file. Dalo's source convention is `instructions/<selector>.md`.

For draft 0.1, the consumer selects the instruction target and scope. The
manifest has no portable project/global scope or nested-directory selector.
A consumer must show where instructions will apply, preserve user-authored
content, and remove only the content it owns. It must not turn project guidance
into global guidance or silently concatenate contradictory provider files.

Dalo writes owned blocks into explicitly selected files. Its provider aliases
map Codex to `$CODEX_HOME/AGENTS.md` and Claude to
`$CLAUDE_CONFIG_DIR/CLAUDE.md`, with their usual user-directory defaults.
An explicit project file can be selected instead. Existing commands and
activation requirements are in the [instruction reference](../reference.md).
These paths and managed-block markers are Dalo implementation details.
The harness remains responsible for reading the file and applying its scope
and precedence rules; identical Markdown is not a promise of identical behavior.

## Local tools

Each `[[tool]]` requires `schema_version`, `id`, `entry`, `runtime`, `argv`,
`cwd`, and `availability`. IDs are unique within the tool namespace.

- `runtime` is `executable`, `node`, or `python`. An executable entry must have
  its executable bit set. `availability` is `required` or `optional`.
- `entry` and optional `files` name regular files inside the package. The entry
  is included automatically and must not be repeated in `files`. Paths cannot
  escape the package or rely on symlinks. Include the complete executable closure.
- `cwd` is currently only `tool_root`: the immutable staged tool directory.
  A project directory can be passed as a bound input; changing cwd is the
  handler's explicit responsibility.
- `inputs` declare a unique name, `type` (`string`, `path`, `integer`, `boolean`),
  and optional `required`, defaulting to true.
- `argv` is an argument vector, never a shell string. Only an entire argument
  such as `${input.project_dir}` may interpolate a declared input. Embedded
  substitutions and undeclared inputs are invalid.
- `env` explicitly lists inherited environment variable names. Other developer
  environment variables are not implicitly passed through.
- `capabilities` declares `filesystem_read`, `filesystem_write`, `subprocess`,
  or `network`. Claims inform review; they do not constitute an OS sandbox.
- Optional `platforms` contains `macos` and/or `linux`; omission is not a claim
  that every platform has the required runtime. `runtime_version` is authored
  metadata, not proof that a version was installed or enforced.

This draft does not define binary downloads, package-manager installation,
update scripts, or installer execution. A reviewed Impeccable engine can be
supplied as a local tool; automatic release acquisition needs a separate design.

## Hooks

Each `[[hook]]` names a same-package `tool` and requires its own `schema_version`
and `id`. It also declares `subject`, `phase`, `effect`, `requirement`,
`timeout_ms`, `failure_policy`, `retry`, `error_visibility`, and `blocking_scope`.
The hook binds event fields to tool inputs; it cannot supply another argv vector.

The current reference implementation admits the following combinations:

| Subject / phase | Effects | Native mapping |
| --- | --- | --- |
| `session` / `end` | `observe` | `SessionEnd` |
| `user_prompt` / `before` | `observe`, `add_context`, `allow_deny` | `UserPromptSubmit` |
| `tool_call` / `before` | `observe`, `add_context`, `allow_deny`, `rewrite_input` | `PreToolUse` |
| `tool_call` / `after` | `observe`, `add_context`; Claude also `replace_output` | Codex `PostToolUse`; Claude also `PostToolUseFailure` |
| `workflow` / `completion_attempt` | `observe`, `continue_workflow` | `Stop` |

This is a bounded provider mapping, not an equivalence claim for all tools or
events. Dalo's pinned adapter baselines are Claude Code 2.1.233 and Codex 0.147.0.
Later versions require evidence rather than an inferred compatibility promise.

`matcher.tool_names` is an optional list of exact native tool names. It is valid
only for tool-call subjects; an empty list means all covered calls. Names are
not a universal taxonomy: `Edit`, `Write`, and `apply_patch` have different inputs.
`bindings` map `{ input, field }` pairs with matching primitive types:

- Strings: `session.id`, `session.permission_mode`, `actor.kind`, `actor.id`,
  `session.end_reason`, `prompt.text`, `tool.call_id`, `tool.name`,
  `workflow.last_message`.
- Paths: `session.cwd`, `transcript.path`.
- Boolean: `workflow.already_continued`.

Fields must exist for the chosen subject/phase. Every required tool input needs
a binding. Missing runtime fields are handled according to the declared failure
policy. The dispatcher also passes raw native event JSON on stdin. That payload
is **not normalized by this draft**. A handler that inspects it needs separate
provider payload tests or an explicit adapter. The example handler uses bound
inputs to avoid that dependency.

Portable stdout is a single JSON result. The supported shapes are:

```json
{"kind":"observe"}
{"kind":"abstain"}
{"kind":"add_context","context":"Review contrast."}
{"kind":"allow"}
{"kind":"deny","reason":"Policy failed."}
{"kind":"rewrite_input","input":{"command":"echo reviewed"}}
{"kind":"replace_output","output":{"result":"reviewed"}}
{"kind":"continue_workflow","reason":"Run the remaining checks."}
```

These are separate examples, not a multi-line response. Results must match the
declared effect; `abstain` is the explicit no-op for any effect. Dalo limits a
handler response to 4 MiB, text fields to 16 KiB without NUL characters, and a
rewritten input object to 1 MiB. A native `hookSpecificOutput` response needs explicit adaptation;
it is not itself portable stdout. A denial does not revoke a user's harness
permissions, and `allow` does not grant permissions the harness withheld.
Post-action effects cannot undo actions. Conflicting controlling results must
not silently select a winner. Dalo composes them in stable hook-identity order.

Timeouts range from 100 to 120000 milliseconds. `retry` is `never` and
`blocking_scope` is `matched_event`. `failure_policy` is `report`, `fail_open`,
or `fail_closed`; the last is valid only for pre-action enforcement or completion
control. Observations and output replacements require `report`.
`error_visibility` declares `user` or `model_and_user`.
Dalo applies the failure policy to unsuccessful exits, timeouts, malformed
results, missing runtime bindings, and outputs incompatible with the declared
effect. `report` and `fail_open` leave the action unchanged and surface a
bounded diagnostic. `fail_closed` denies a pre-action gate or requests another
workflow pass. A user-only failure exposes the detailed diagnostic through the
native user channel and only a generic explanation when a control result needs
a model-facing reason. Handler stderr is never copied into model context.

Provider support includes these audience rules. Neither adapter can deliver
model-visible diagnostics after `SessionEnd`; such descriptors are reported as
unsupported. User-only failures there use a nonzero dispatcher exit because
the event ignores ordinary context output. Claude's `Stop` context resumes the
turn, so a nonblocking `report` or `fail_open` failure cannot promise
`model_and_user` delivery there. Use `user`, or `fail_closed` when continuation
is intended. Required unsupported contracts block activation; optional ones
with an explicit omission fallback remain visibly omitted.

Input rewriting preserves Claude's ordinary permission flow. Codex requires
the native `allow` field alongside a rewrite; its independent permission and
sandbox checks still apply. Output replacement targets Claude `PostToolUse`
only, since its failure event accepts context but no replacement. The native
`stop_hook_active` flag remains available through `workflow.already_continued`;
handlers decide when to abstain to avoid repeated continuation. See the
[compatibility matrix](compatibility.md) for evidence and provider limits.

An optional hook must explicitly declare `fallback = "omit"`. Required hooks
must not declare a weakening fallback. Unsupported required enforcement must
block activation; it cannot be replaced by advisory text.

## Validation, activation, and interoperability

Consumers should distinguish three results: valid package structure, supported
behavior for a named provider/version, and authorized activation. Success at
one stage does not imply success at the next.

Execution requires explicit user trust covering the tool bytes and invocation
contract, plus the hook's event binding and intended effect. Consumers choose
their own trust storage. Dalo stages immutable tool files and independently
checks the exact tool and hook grants, including at dispatch time. A consumer
must never infer execution approval from downloading or selecting a package.

Consumers must bound untrusted parsing and file traversal. Dalo's reference
limits are 1 MiB per manifest, 16 KiB per string, 1024 array entries, 32 levels
of TOML nesting, 4096 package entries, 32 directory levels, 64 MiB per file,
and 256 MiB per package. Hook matchers and bindings have a tighter 256-entry
limit. JSON Schema string lengths count characters, so byte limits still need
semantic validation.

The [conformance cases](../../tests/package_spec.rs) exercise the structural
schema and production parser against shared examples, distinguishing structural
rejection from local semantic failures. They also check reference scope and
dependency cycles. The [CLI cases](../../tests/package_validate_cli.rs) exercise
validation without a store. The [package lifecycle cases](../../tests/package_lifecycle.rs)
cover selection, review, approval, installation, tool-byte changes, revocation,
and removal, with independently activated instruction blocks and preserved
user content on both provider targets.
The [upstream integration guide](../../tests/fixtures/upstream-hooks/README.md)
distinguishes original hook execution from recordings and live engine tests.

| Upstream case | What is established | Work before full package support |
| --- | --- | --- |
| Impeccable | Recorded post-edit output round trips; opt-in real engine finds contrast issues without its installer. | Stop deep pass, lifecycle behavior, engine acquisition, and native input coverage. |
| Get Shit Done | Its original pre-tool guard produces advisory context for tested Claude payloads. | Other hooks, bootstrap, runtime dependencies, and Codex tool payloads. |
| Planning with Files | Its original Cursor reminder can run through an explicit context adapter. | Its distinct Codex wrappers, session start, compaction, permission, and stop semantics. |

These adaptations are maintained here; none is an upstream author's endorsement
of this draft. A green fixture suite is not complete package certification.

## Route to a stable revision

Before declaring 1.0, exercise full integrations with at least three independent
packages and obtain author feedback on the metadata and maintenance burden.
Each proposal should include an upstream revision, required behavior, provider
versions, executable evidence, and unsupported cases. Another consumer should
be able to implement the contract without reproducing Dalo's internal state.

Open design questions include normalized tool-event payloads, declarative
instruction scope, additional lifecycle events, runtime acquisition, and provider
overlay semantics. MCP servers remain outside this draft. Additions need a
versioned contract and tests; they must not reinterpret an existing schema 1
manifest silently. Until there is joint adoption, keep the specification and
reference implementation together and avoid claiming ecosystem governance.

# Upstream hook integration fixtures

`tests/upstream_hooks.rs` exercises pinned third-party hook behavior through
Dalo's real tool approval, immutable staging, hook approval, native sidecar
merge, and process dispatcher. No upstream installer runs. The ordinary suite
uses local fixtures and needs Node.js and Bash on macOS or Linux:

```sh
cargo test --locked --test upstream_hooks
```

These tests run with the ordinary Cargo suite in CI. They do not launch Claude,
Codex, Cursor, or a model. Native-provider assertions check Dalo's emitted JSON,
not delivery into a live conversation. Windows process execution is not covered.

## What actually runs

| Package | Ordinary offline test | Additional evidence and limits |
| --- | --- | --- |
| [Impeccable](https://github.com/pbakaus/impeccable/tree/cd12f8660e2dde57b9615c8a6b8ea674101f9cfc) | Replays the author's exact successful `PostToolUse` recordings for a problematic HTML page and clean TSX file; compares the complete emitted native JSON for both Claude and Codex. | The opt-in test below runs the real engine against the original HTML workspace. Recording replay alone does not test the design engine. The `Stop` deep pass is not covered. |
| [Get Shit Done](https://github.com/gsd-build/get-shit-done/tree/bdcaab2c752d9a33a1a1ca9acf3a3c81fb991815) | Executes the original Node `gsd-prompt-guard.js` against suspicious and clean planning text using a Claude `PreToolUse` payload. | Asserts advisory context, no permission decision, and no file write. This does not cover GSD's installer, other hooks, or Codex `apply_patch` payloads. |
| [Planning with Files](https://github.com/OthmanAdi/planning-with-files/tree/f67e2bb7294867c37bbea150de0bf81f2c3449d3) | Executes the original Cursor Bash reminder with and without `task_plan.md`, explicitly adapts its plain text to portable context, and checks Codex output. | Demonstrates reuse of the reminder logic and correct project cwd. It does not test Cursor's hook protocol or Planning with Files' different Codex Python wrappers. |

The lifecycle test also checks separate approvals, dry-run, idempotent sidecar
installation, preservation of foreign settings, revocation of either grant
against an already installed projection, and removal of only Dalo-owned groups.

## Explicit adaptation, not automatic import

The suite authors a small portable `PLUGIN.toml` for each selected hook. Its
tool closure includes the original handler (or recording/engine), the test
adapter, and a launcher pinned to the test machine's Node executable. Dalo
approves and stages those bytes using the production APIs.

`context-adapter.mjs` is test infrastructure, not a shipped package importer.
It invokes the original handler in the temporary project with an explicit
environment and translates only event-matching native `additionalContext`
into Dalo's portable `add_context` result. Tests ensure control decisions and
wrong-event responses are rejected, rather than silently weakened to advice.
Child failures also fail the adapter; the authored advisory hook uses Dalo's
existing `failure_policy = "report"` behavior.

The preserved upstream manifests document the wider integration gap:

- Impeccable also registers `Stop` and has a separate Cursor configuration.
- Planning with Files registers `SessionStart`, `PreCompact`, and
  `PermissionRequest`, which Dalo's current portable event vocabulary cannot
  represent. Its stop behavior needs separate effect analysis.
- Shell launcher paths, harness environment variables, payload differences,
  and output contracts are not made portable merely by copying `SKILL.md`.

Passing these cases therefore does not mean the complete packages can already
be imported or installed by Dalo. New event support still needs the explicit
schema and provider-contract work described in RFC 0005.

Native context output follows the event envelopes documented by
[Claude Code](https://code.claude.com/docs/en/hooks#add-context-for-claude) and
[Codex](https://learn.chatgpt.com/docs/hooks). The tests use Dalo's existing
provider baselines; they do not expand that version claim.

## Run the actual Impeccable engine

Obtain a reviewed platform binary from the author's
[engine releases](https://github.com/pbakaus/impeccable/releases/tag/engine-v0.1.5)
and verify its published SHA-256. No installation or launcher is needed. Supply
the binary and expected digest explicitly:

```sh
DALO_TEST_IMPECCABLE_BIN=/absolute/path/to/impeccable-darwin-arm64 \
DALO_TEST_IMPECCABLE_SHA256=0d48b6e16aa97664fdbe607d5da9ae320e389a1843e0ff1328f8c2530530320d \
cargo test --locked --test upstream_hooks impeccable_engine_runs -- --ignored
```

The example digest is for `engine-v0.1.5`'s `impeccable-darwin-arm64`, exercised
on macOS ARM64. Other platforms require their own matching binary and digest.
The test checks the digest before execution, stages the binary as a Dalo tool,
invokes its `hook` command through Dalo, and asserts actual contrast findings
and the project cache. It also checks that no upstream installation directory
or downloaded launcher binary appears. The binary is not committed, and the
test performs no download. Its HOME and cache root are inside the temporary
project. The explicit environment is isolation from developer configuration,
not an OS security sandbox for an untrusted executable.

This test is ignored by default because the regular suite must remain offline
and must not fetch or execute a newly released binary without review.

## Provenance and updates

`provenance.json` records the repository, full commit, original path, and
SHA-256 of every vendored upstream file. Files are copied byte for byte and
retain their upstream licenses and, for Impeccable, its notice. The integrity
test catches accidental fixture edits; it does not authenticate a release.

To update a package, review the new upstream revision, copy the selected files
without formatting them, update its provenance entries, and rerun the suite.
Review changed manifests and output semantics even if the tests still pass.
For Impeccable, rerun the opt-in engine test and record the reviewed release
and platform digest here. Add new cases with an explicit distinction between
original process execution, recorded output, and an untested capability.

# RFC 0006: Verified Release Binaries for Skills

Status: Accepted, recorded in [ADR 0011](../adr/0011-verified-release-binaries.md)  
Date: 2026-10-09  
Author: Sebastian + Claude  
Depends on: RFC 0002, RFC 0003, RFC 0005  
Related: [#937](https://github.com/sebastian-software/dalo/issues/937),
[#934](https://github.com/sebastian-software/dalo/issues/934)  
Discussion: https://github.com/sebastian-software/dalo/issues/937

## 1. Summary

Some skills need a native binary per platform. Today each such skill ships its
own downloader: Impeccino's `scripts/impeccino` launcher picks an asset from a
GitHub release, fetches it with curl, compares it with a checksum file from the
same release, and caches it under the user's home directory. That is
hand-rolled supply-chain code inside a skill, and Dalo cannot pin, approve,
audit, or remove the bytes it fetches.

This RFC lets a skill declare the release binaries it needs in its `SKILL.md`
frontmatter. Dalo fetches only the host platform's asset, verifies it against a
digest pinned in the source, stages it immutably in the store, records it in
the lock, and exposes it through one stable path. Identity, approval,
verification, and inspection reuse the local-tool model of RFC 0005:
content-bound identity, exact approval, inert discovery, read-only inspection.

Four questions were open in #937 and are decided here: the declaration lives
in the skill frontmatter rather than in a plugin manifest or sidecar; Dalo
fetches and verifies natively on dependencies it already ships rather than
delegating to mise or aqua; the trust anchor is the SHA-256 digest pinned in
the reviewed source, without provenance attestation; and a ready binary is
exposed at a fixed path inside the store.

## 2. Motivation

The weaknesses of per-skill downloaders repeat in every skill that needs a
binary:

1. **Dalo pins the skill commit, not the binary bytes.** A checksum file
   published next to the asset guards against corruption, not against a
   replaced asset. A pin that lives in the source, reviewed with the skill,
   closes that gap: a skill commit plus the lock then names the exact bytes.
2. **Nothing is audited or approved.** `[[tool]]` contracts pin plugin-local
   executables by hash and closure, but a binary fetched at runtime bypasses
   approvals, `dalo audit`, and `dalo doctor` entirely.
3. **Every skill reinvents it**, with its own cache layout, platform detection,
   proxy handling, and failure modes. agent-browser, needed by Impeccable's
   rendered-page scans (#934), is another binary users install by hand.

## 3. Goals

- Let a skill declare per-platform release binaries with pinned digests, in
  the one file its author already maintains.
- Fetch only what the host needs, verify before anything is renamed into
  place, and store it content-addressed and immutable.
- Make the pinned digests part of what a user approves; a changed digest needs
  a fresh approval, exactly like a changed tool contract.
- Report declared binaries and their verification state in `dalo binary
  list|show` and `dalo doctor`, and name them when a skill is approved.
- Expose each ready binary at one stable path a skill launcher can find
  without knowing Dalo's internals.
- Keep every inspection command free of network and execution side effects.
- Add no new crate for the digest path.

## 4. Non-goals

- Running the binary. Dalo verifies bytes; the agent runs the skill.
- Resolving versions, dependency graphs, or update channels. A declaration
  names one tag and one digest per platform; updating is a source change.
- Supporting arbitrary download sources. The first cut supports GitHub
  release assets only; the `source` field leaves room for more kinds.
- Archive extraction. An asset is one executable file. Archives can follow as
  a separate asset kind once there is a concrete case.
- Provenance attestation (Sigstore bundles, `gh attestation verify`). The
  digest pinned in a reviewed source is the trust anchor; see section 7.
- Windows. Dalo has no native Windows support (#830); the platform set is the
  one `[[tool]]` already uses, split by architecture.
- Project scope in the first cut. Binaries are approved per store; a project
  store gets them once the project declaration can name them.

## 5. Design

### 5.1 Declaration: `binaries` in the `SKILL.md` frontmatter

```yaml
---
name: impeccino
description: Audit rendered pages with the Impeccino engine
binaries:
  impeccino:
    source: github-release
    repo: sebastian-software/impeccino
    tag: engine-v0.2.0
    availability: required
    assets:
      macos-arm64:
        asset: impeccino-darwin-arm64
        sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
      linux-x64:
        asset: impeccino-linux-x64
        sha256: "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
---
```

The declaration is a top-level frontmatter field in the style of `requires`:

- The author declares it in the file they already maintain, without a plugin
  package or a second Dalo-owned file next to `SKILL.md`.
- Dalo already reads its own frontmatter fields (`id`, `owners`, `tags`,
  `requires`). Agent runtimes ignore unknown fields; only strict lint tools
  report them, and they already report Dalo's existing fields. Dalo's own
  specification check lists `binaries` among its known fields.
- The declaration is part of the skill's content hash, so `dalo audit`, the
  content-bound skill approval, and catalog drift all see a changed binary.
- A skill that needs a Dalo-managed binary is still an ordinary skill
  everywhere else: its launcher falls back to its own download or to `PATH`
  on hosts without Dalo.

The schema is closed. Rules:

| Field | Rule |
| --- | --- |
| `binaries` | mapping from binary id to declaration; an empty mapping declares nothing |
| id (mapping key) | lower kebab-case; its own `#binary:` namespace within the skill |
| `source` | `github-release`; a closed enum so other kinds are added deliberately |
| `repo` | `<owner>/<name>`, two segments of `[A-Za-z0-9._-]`, neither `.` nor `..`, no leading `.` |
| `tag` | 1–128 characters of `[A-Za-z0-9._/-]`, no leading `-`, `.`, or `/`, no trailing `/`, no `..` or `//` |
| `availability` | optional; `required` (default) or `optional`, with the tool semantics |
| `assets.<platform>` | one of `macos-arm64`, `macos-x64`, `linux-arm64`, `linux-x64`; at least one entry |
| `assets.<platform>.asset` | release asset file name, `[A-Za-z0-9._-]`, no leading `.`, no `..` |
| `assets.<platform>.sha256` | 64 lowercase hex characters |

An invalid declaration drops the skill from the inventory with an
`invalid_binary_declaration` warning, exactly as an invalid `DELIVERY.toml`
does. Assets and digests are explicit per platform; there is no file-name
template. The download URL is derived and never authored:
`https://github.com/<repo>/releases/download/<tag>/<asset>`.

### 5.2 Identity and approval

A binary's identity is `<source>:<slot>#binary:<id>`, where `<source>:<slot>`
is the skill's source-qualified ref. Its contract hash is a SHA-256 over the
framed fields `id`, `source`, `repo`, `tag`, `availability`, and every
`(platform, asset, sha256)` triple in platform order. The approval value is

```text
binary  <source>:<slot>#binary:<id>@sha256:<contract-hash>
```

The contract covers every platform's digest, so one approval on macOS also
approves the Linux digest of the same declaration. That is deliberate: the
reviewer approves a declaration they can read in the source, not a byte string
they cannot. The bytes themselves are checked against the host platform's
pinned digest at fetch time and at every later inspection. Changing any digest,
the tag, or the repository changes the contract hash and lands as `hash drift`
until approved again. Approving the skill does not approve its binaries, and
`dalo approve skill` names the declared binaries so the next step is visible.
There is no wildcard scope.

### 5.3 Fetch, verify, stage

`dalo approve binary <identity>` is the only command that reaches the network.
It:

1. requires the exact declaration to be the one the user is approving, the
   host platform to be declared, and the skill to be present in an enabled
   source's inventory;
2. downloads the host asset over HTTPS to a temporary file inside the store,
   hashing while streaming, with a bounded size (256 MiB) and timeout,
   following redirects only to `github.com` and
   `objects.githubusercontent.com`, and never sending credentials;
3. compares the digest with the pinned one and deletes the temporary file on
   any mismatch, reporting `binary verification failed` without creating an
   approval record;
4. renames the file into `binaries/<sha256>/<id>` with mode `0555` and makes
   the directory read-only, like `tools/`;
5. writes the approval record;
6. creates or updates the exposure path (5.4).

Fetching is content-addressed: a digest that is already staged is not fetched
again, which also means a second skill pinning the same bytes shares them.
Interrupted downloads leave only a temporary file that `dalo doctor` reports
as `binary_staging_debris`, mirroring tool staging.

Transport uses `ureq` with rustls, which is already compiled into Dalo for the
update check; it becomes a direct dependency without adding a crate. Hashing
uses the existing `sha2` dependency. Proxy support follows `ureq`'s standard
`HTTPS_PROXY` handling.

### 5.4 Exposure

A ready binary is exposed at `<store>/bin/<id>`, a symlink to the staged file.
The path is stable across digest changes, so a launcher needs to know only the
store location:

```sh
#!/bin/sh
# Launcher inside the skill: prefer the Dalo-verified binary, fall back to the
# skill's own download for hosts without Dalo.
for candidate in "${DALO_STORE:-$HOME/.dalo}/bin/impeccino" "./.dalo/bin/impeccino"; do
  [ -x "$candidate" ] && exec "$candidate" "$@"
done
exec "$(dirname "$0")/impeccino-fallback" "$@"
```

Dalo cannot set environment variables in the agent's process, so the
`expose_env` idea from the issue is not part of this design. The documented
lookup contract replaces it: `DALO_STORE` when set, `~/.dalo/bin/` otherwise,
plus the project store's `.dalo/bin/` once project scope supports binaries.

Two skills declaring the same id compete for one path. The first skill by
source priority wins, the other is reported as `binary_slot_conflict` by
`dalo doctor` and its binary stays `blocked`, the same way skill slots handle
conflicts today.

### 5.5 States, inspection, doctor

`dalo binary list|show` mirrors `dalo tool list|show` and never downloads. The
state machine is the tool one: `platform_unsupported`, `pending_approval`,
`hash_drift`, `revoked`, `approved_not_staged`, `audit_failure`, `ready`, plus
`blocked` for an exposure conflict. `ready` requires an exact approval, staged
bytes that re-hash to the pinned digest, and an exposure link pointing at
them.

`dalo doctor` gains `binary_pending_approval`, `binary_hash_drift`,
`binary_platform_unsupported` (info for optional binaries, warning for
required ones), `binary_approval_revoked`, `binary_audit_failed`,
`binary_staging_debris`, `binary_slot_conflict`, and `binary_ready`.

`dalo audit <source:skill>` stays a content audit of the skill directory and
does not fetch. Because the declaration is frontmatter, the audit already
covers it, and its report lists the declared binaries informationally so a
reviewer sees that approving the skill is not the whole story.

### 5.6 Lock and garbage collection

`lock.toml` records every staged binary under an additive `binaries[]` list:
`source_ref`, `contract_hash`, `platform`, `digest`, `staged_path`, and
`exposed_path`. The schema version does not change; older Dalo versions ignore
the list and newer ones treat an empty list as "nothing staged".

A staged digest is removed when no approval record references a declaration
pinning it: on `approve revoke binary`, on `source unselect` or `source remove`
of the declaring skill. The exposure link is removed together with the last
staged digest for its id. Nothing under `binaries/` is ever modified in place.

## 6. Why native rather than mise or aqua

Delegating download and verification to mise's `github` or `aqua` backend was
the alternative the issue raised. It is rejected:

- **It adds a runtime dependency to every Dalo user.** mise is a full tool
  manager with its own configuration, plugins, and trust model. Requiring it
  for one feature changes what installing Dalo means.
- **It moves the pin out of Dalo's lock.** mise keeps its own lockfile and
  cache layout; `dalo doctor`, `dalo audit`, and the approval ledger would
  describe state they do not own.
- **The native path is small and stands on existing shoulders.** Streaming a
  file over HTTPS and comparing a SHA-256 is a few hundred lines on `ureq`,
  rustls, and `sha2`, all already in the dependency tree. No new crate is
  introduced.
- **The posture stays the same.** `[[tool]]` approval validates and stages
  bytes without running them; binary approval does the same with bytes that
  arrive over the network instead of from the checkout.

mise and aqua remain the right answer for users who manage their toolchains
that way; a skill launcher that checks `PATH` after the Dalo path keeps that
option open.

## 7. Alternatives considered

- **Keep per-skill launchers.** Works without Dalo, but each skill carries its
  own supply-chain code, and Dalo cannot pin, approve, or remove the bytes.
- **Vendor binaries in the skill repository.** Every platform in every clone,
  binaries in Git history, large repositories.
- **Declare binaries as `[[binary]]` in `PLUGIN.toml`.** This was built first.
  It reuses the plugin review session, but it forces every skill with a binary
  into a plugin package, and users then select and review a plugin instead of
  approving a skill. The approval machinery is the same either way, so the
  wrapper buys nothing the frontmatter cannot provide.
- **Declare binaries in `DELIVERY.toml`.** That file describes which content
  variant each provider receives and is bound to a `kind`; a runtime
  dependency does not belong there, and a skill without provider variants
  would need a `kind` only to carry it.
- **Declare binaries as flat `metadata` keys.** Conformant with the Agent
  Skills specification, but up to ten near-identical string entries per
  binary and no structure for the validator to check.
- **Expose through an environment variable.** Dalo does not control the
  agent's environment; a documented stable path does the same job.
- **`[[tool]]` with a remote entry.** A tool is invoked by hooks through Dalo's
  dispatcher; a binary is invoked by the skill. A later revision can let a
  tool's `entry` reference a ready binary.
- **Provenance attestation.** Verifying a Sigstore bundle natively would pull a
  large certificate and signature stack into Dalo for one check; delegating
  to the GitHub CLI would make a ready binary depend on another installed
  tool. The pinned digest in a reviewed source already names the exact bytes;
  a replaced upstream asset fails verification. Attestation can be revisited
  as an additive `attestation` field if a concrete case needs it.

## 8. Delivery slices

1. **Declaration and read-only inventory.** `binaries` parsing and validation
   in the inventory, `BinaryRecord` with contract hash, `dalo binary
   list|show`, documentation. No network.
2. **Approval, fetch, verify, stage, expose.** `dalo approve binary`,
   `approve revoke binary`, staging under `binaries/`, exposure under `bin/`,
   doctor findings, the binary list in `approve skill` output, offline tests
   against a local HTTP fixture server (matching digest, tampered asset,
   oversized asset, redirect outside the allowed hosts, changed digest
   requiring re-approval, unsupported platform reported without failing the
   sync).
3. **Lock, audit, cleanup.** `lock.toml` records, the informational binary list
   in `dalo audit`, garbage collection on revoke and removal, `security.md`
   guidance, launcher contract documentation.

## 9. Open questions

- Should `availability = "required"` hold back the skill on an unsupported
  host, or only report it? Tools block their plugin; a skill usually degrades,
  so the first cut reports and does not block.
- Archive assets (`.tar.gz`, `.zip`) with an inner path, for projects that only
  publish archives.

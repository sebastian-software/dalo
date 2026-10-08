# RFC 0006: Verified Release Binaries for Plugins and Skills

Status: Proposed  
Date: 2026-10-08  
Author: Sebastian + Claude  
Depends on: RFC 0002, RFC 0005  
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

This RFC lets a plugin declare the release binaries it needs. Dalo fetches
only the host platform's asset, verifies it against a digest pinned in the
source, stages it immutably in the store, records it in the lock, and exposes
it through one stable path. Declaration, approval, verification, and exposure
reuse the local-tool model of RFC 0005: content-bound identity, exact
approval, inert discovery, read-only inspection.

The open question in #937, whether Dalo should implement verification itself
or delegate to mise or aqua, is decided here in favor of a small native
implementation for the digest path, with provenance attestation delegated to
an optional external verifier in a later slice.

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

Established tooling already solves parts of this: GitHub immutable releases
lock assets after publishing, `actions/attest-build-provenance` binds a digest
to repository, workflow, and commit, and mise and aqua verify checksums and
attestations natively. Dalo should reuse those guarantees without becoming a
general package manager.

## 3. Goals

- Let a plugin declare per-platform release binaries with pinned digests.
- Fetch only what the host needs, verify before anything is renamed into
  place, and store it content-addressed and immutable.
- Make the pinned digest part of what a user approves; a changed digest needs
  a fresh approval, exactly like a changed tool contract.
- Report declared binaries and their verification state in `dalo binary
  list|show`, `dalo doctor`, and `dalo plugin review`.
- Expose each ready binary at one stable path a skill launcher can find
  without knowing Dalo's internals.
- Keep every inspection command free of network and execution side effects.

## 4. Non-goals

- Running the binary. Dalo verifies bytes; the agent runs the skill.
- Resolving versions, dependency graphs, or update channels. A declaration
  names one tag and one digest per platform; updating is a source change.
- Supporting arbitrary download sources. The first cut supports GitHub
  release assets only; the descriptor leaves room for more kinds.
- Archive extraction. An asset is one executable file. Archives can follow as
  a separate asset kind once there is a concrete case.
- Windows. Dalo has no native Windows support (#830); the platform set is the
  one `[[tool]]` already uses, split by architecture.
- Project scope. ADR 0010 limits project installations to skills; binaries
  belong to plugins and therefore to global stores until project scope grows
  plugin support.

## 5. Design

### 5.1 Declaration: `[[binary]]` in `PLUGIN.toml`

```toml
[[binary]]
schema_version = 1
id = "impeccino"
source = "github-release"
repo = "sebastian-software/impeccino"
tag = "engine-v0.2.0"
availability = "required"

[binary.assets.macos-arm64]
asset = "impeccino-darwin-arm64"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[binary.assets.linux-x64]
asset = "impeccino-linux-x64"
sha256 = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
```

The declaration lives next to `[[tool]]` in the plugin manifest rather than in
`SKILL.md` or `DELIVERY.toml`:

- The Agent Skills specification has no field for it, and `SKILL.md`
  frontmatter must stay portable to every runtime that reads it.
- `DELIVERY.toml` describes how one skill's content is built or selected per
  provider. A binary is not content of the skill; it is an active component
  the skill depends on, and RFC 0005 already puts active components into the
  plugin package with their own approvals.
- The plugin review session already walks tools and hooks one decision at a
  time; a binary is one more decision with the same shape.

The schema is closed: unknown fields reject the package, like every other
descriptor. Rules:

| Field | Rule |
| --- | --- |
| `schema_version` | exactly `1` |
| `id` | lower kebab-case, unique among the plugin's binaries; its own `#binary:` namespace |
| `source` | `github-release`; a closed enum so other kinds can be added deliberately |
| `repo` | `<owner>/<name>`, two segments of `[A-Za-z0-9._-]`, neither `.` nor `..` |
| `tag` | 1–128 characters of `[A-Za-z0-9._/-]`, no leading `-` or `.`, no `..` or `//` |
| `availability` | `required` or `optional`, with the tool semantics: a required binary that is not ready blocks the plugin for the host, an optional one is reported |
| `assets.<platform>` | one of `macos-arm64`, `macos-x64`, `linux-arm64`, `linux-x64`; at least one entry |
| `assets.<platform>.asset` | release asset file name, `[A-Za-z0-9._-]`, no leading `.`, no `..` |
| `assets.<platform>.sha256` | 64 lowercase hex characters |

Assets and digests are explicit per platform. The issue sketched a template
(`impeccino-{os}-{arch}{exe}`) with a digest table; the explicit form needs no
placeholder vocabulary, cannot name an asset without a digest, and reads the
same way the lock records it. The download URL is derived and never authored:
`https://github.com/<repo>/releases/download/<tag>/<asset>`.

### 5.2 Identity and approval

A binary's identity is `<source>:<plugin>#binary:<id>`. Its contract hash is a
SHA-256 over the framed fields `schema_version`, `id`, `source`, `repo`,
`tag`, `availability`, and every `(platform, asset, sha256)` triple in platform
order. The approval value is

```text
binary  <source>:<plugin>#binary:<id>@sha256:<contract-hash>
```

The contract covers every platform's digest, so one approval on macOS also
approves the Linux digest of the same declaration. That is deliberate: the
reviewer approves a declaration they can read in the source, not a byte string
they cannot. The bytes themselves are checked against the host platform's
pinned digest at fetch time and at every later inspection. Changing any digest,
the tag, or the repository changes the contract hash and lands as `hash drift`
until approved again. There is no wildcard scope.

### 5.3 Fetch, verify, stage

`dalo approve binary <ref>` is the only command that reaches the network. It:

1. requires the exact declaration to be the one the user is approving, the
   host platform to be declared, and the plugin package to be accepted by the
   inventory;
2. downloads the host asset over HTTPS to a temporary file inside the store,
   hashing while streaming, with a bounded size (256 MiB) and timeout, following
   redirects only to `github.com` and `objects.githubusercontent.com`, and
   never sending credentials;
3. compares the digest with the pinned one and deletes the temporary file on
   any mismatch, reporting `binary verification failed` without creating an
   approval record;
4. renames the file into `binaries/<sha256>/<id>` with mode `0555` and makes
   the directory read-only, like `tools/`;
5. writes the approval record;
6. creates or updates the exposure path (5.4).

Fetching is content-addressed: a digest that is already staged is not fetched
again, which also means a second plugin pinning the same bytes shares them.
Interrupted downloads leave only a temporary file that `dalo doctor` reports
as `binary_staging_debris`, mirroring tool staging.

Transport uses `ureq` with rustls, already in the dependency tree through the
update check. Proxy support follows `ureq`'s standard `HTTPS_PROXY` handling.
No new TLS or crypto dependency is introduced for the digest path.

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
plus the project store's `.dalo/bin/` once project scope supports plugins.

Two plugins declaring the same `id` compete for one path. The first plugin by
source priority wins, the other is reported as `binary_slot_conflict` by
`dalo doctor` and its binary stays `blocked`, the same way skill slots and
plugin projections handle conflicts today.

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
`dalo plugin review` adds one `binary` decision per declared binary, approved
through the same atomic entry as tools and hooks; the download happens when
the decision is committed, and a verification failure fails the whole review
commit before any approval is written.

`dalo audit <source:skill>` stays a content audit of the skill directory and
does not fetch. Its report lists, informationally, the binaries declared by
plugins that include the skill as a member, so a reviewer sees that approving
the skill is not the whole story.

### 5.6 Lock and garbage collection

`lock.toml` records every staged binary under an additive `binaries[]` list:
`source_ref`, `contract_hash`, `platform`, `digest`, `staged_path`, and
`exposed_path`. The schema version does not change; older Dalo versions ignore
the list and newer ones treat an empty list as "nothing staged".

A staged digest is removed when no approval record references a declaration
pinning it: on `approve revoke binary`, on `plugin unselect` when the user
confirms the cleanup, and on `source remove`. The exposure link is removed
together with the last staged digest for its `id`. Nothing under `binaries/`
is ever modified in place.

### 5.7 Provenance attestation (later slice)

A declaration may add an attestation identity:

```toml
attestation = { repo = "sebastian-software/impeccino", workflow = ".github/workflows/release-engine.yml" }
```

When present, approval additionally requires a successful offline verification
of the asset's Sigstore bundle against that identity. Dalo stores the bundle
next to the staged binary and verifies through `gh attestation verify --bundle`
when the GitHub CLI is installed; a declared attestation with no verifier
available keeps the binary `pending_verification`, never `ready`. This is the
same posture as the optional agent reviewer: an external CLI adds a stronger
check, its absence degrades loudly rather than silently. Implementing Sigstore
verification natively would pull a large crypto stack into Dalo for one check
and is not proposed.

## 6. Why native rather than mise or aqua

Delegating download and verification to mise's `github` or `aqua` backend was
the alternative the issue raised. It is rejected for the first cut:

- **It adds a runtime dependency to every Dalo user.** mise is a full tool
  manager with its own configuration, plugins, and trust model. Requiring it
  for one feature changes what installing Dalo means.
- **It moves the pin out of Dalo's lock.** mise keeps its own lockfile and
  cache layout; `dalo doctor`, `dalo audit`, and the approval ledger would
  describe state they do not own.
- **The native path is small.** Streaming a file over HTTPS and comparing a
  SHA-256 is a few hundred lines on dependencies Dalo already ships. The
  expensive part, provenance verification, is delegated to the GitHub CLI
  exactly as mise delegates nothing but as `dalo audit --reviewer` already
  delegates to agent CLIs.
- **The posture stays the same.** `[[tool]]` approval validates and stages
  bytes without running them; `[[binary]]` approval does the same with bytes
  that arrive over the network instead of from the checkout.

mise and aqua remain the right answer for users who manage their toolchains
that way; a skill launcher that checks `PATH` after the Dalo path keeps that
option open.

## 7. Alternatives considered

- **Keep per-skill launchers.** Works without Dalo, but each skill carries its
  own supply-chain code, and Dalo cannot pin, approve, or remove the bytes.
- **Vendor binaries in the skill repository.** Every platform in every clone,
  binaries in Git history, large repositories.
- **Declare binaries in `DELIVERY.toml`.** Mixes provider build selection with
  dependency fetching and has no approval identity of its own.
- **Declare binaries in `SKILL.md` metadata.** `metadata` values are strings;
  a per-platform digest table does not fit, and it would push a Dalo-specific
  active descriptor into the portable spec surface. #934 uses `metadata` for
  the narrow, checkable case of "this command must be on PATH", which stays
  the right tool for binaries the user installs by hand.
- **Expose through an environment variable.** Dalo does not control the
  agent's environment; a documented stable path does the same job.
- **`[[tool]]` with a remote entry.** A tool is invoked by hooks through Dalo's
  dispatcher; a binary is invoked by the skill. Reusing the identity model
  without merging the descriptors keeps the hook contract unchanged. A later
  revision can let a tool's `entry` reference a ready binary.

## 8. Delivery slices

1. **Declaration and read-only inventory.** `[[binary]]` parsing and validation,
   `BinaryRecord` with contract hash, `dalo binary list|show`, package schema
   and `plugin validate` support, documentation. No network. Tracked by the
   first pull request linked from #937.
2. **Approval, fetch, verify, stage, expose.** `dalo approve binary`,
   `approve revoke binary`, staging under `binaries/`, exposure under `bin/`,
   doctor findings, offline tests against a local HTTP fixture server
   (matching digest, tampered asset, oversized asset, redirect outside the
   allowed hosts, changed digest requiring re-approval, unsupported platform
   reported without failing the sync).
3. **Lock, review, audit, cleanup.** `lock.toml` records, `plugin review`
   decisions, the informational binary list in `dalo audit`, garbage collection
   on revoke and removal, `security.md` and `plugins.md` guidance, launcher
   contract documentation.
4. **Attestation.** Optional `attestation` identity verified through the GitHub
   CLI, bundle storage, `pending_verification` state.

Slice 1 is acceptable before this RFC is accepted because it is inert; slices
2 to 4 wait for the ADR that records the decision.

## 9. Open questions

- Should `availability = "required"` block the whole plugin on an unsupported
  host, or only the components that name the binary? The tool semantics block
  the plugin; a finer binding (skill member → binary) may be worth it once
  there is more than one binary per plugin.
- Should the exposure directory be shared across stores (for example
  `~/.local/bin`)? Keeping it inside the store keeps ownership and removal
  simple; a user-controlled `PATH` entry pointing at `~/.dalo/bin` is a
  documentation matter.
- Archive assets (`.tar.gz`, `.zip`) with an inner path, for projects that only
  publish archives.

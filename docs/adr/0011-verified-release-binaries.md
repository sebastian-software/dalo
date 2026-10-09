# ADR 0011: Verified Release Binaries Declared in Skill Frontmatter

Status: Accepted  
Date: 2026-10-09  
Source: [RFC 0006: Verified Release Binaries for Skills](../rfcs/0006-verified-release-binaries.md)  
Related: [Issue 937](https://github.com/sebastian-software/dalo/issues/937)

## Context

Some skills need a native binary per platform, and each one ships its own
downloader with its own cache, platform detection, and failure modes. Dalo pins
the skill commit but not the bytes such a downloader fetches, nothing about
them is approved or audited, and a checksum published next to the asset guards
against corruption, not against a replaced asset.

## Decision

A skill declares the release binaries it needs in a top-level `binaries`
frontmatter field of `SKILL.md`, keyed by binary id, with one GitHub release
(`repo`, `tag`) and, per platform, the asset file name and its SHA-256 digest.

- The declaration is a Dalo field like `requires`, part of the skill's content
  hash, and closed: an invalid declaration drops the skill with an
  `invalid_binary_declaration` warning.
- Each binary has its own content-bound identity
  `<source>:<slot>#binary:<id>@sha256:<contract-hash>` and its own exact
  approval, separate from the skill's. A changed digest, tag, or repository is
  hash drift until approved again.
- Dalo fetches and verifies natively: only the host platform's asset, over
  HTTPS on the `ureq` and rustls stack it already ships, hashed while
  streaming and compared with the pinned digest before anything is renamed
  into place. No new crate, no delegation to mise or aqua.
- The pinned digest in the reviewed source is the trust anchor. Provenance
  attestation is not part of the design.
- Verified bytes are staged immutably under `binaries/<sha256>/` and exposed
  at the fixed path `<store>/bin/<id>`. Launchers look there (honoring
  `DALO_STORE`) and keep their own fallback for hosts without Dalo.
- Inspection (`dalo binary list|show`, `dalo doctor`, `dalo audit`) never
  downloads or executes anything.

## Consequences

- A skill commit plus the lock names the exact bytes a machine runs; replacing
  an upstream asset fails verification instead of being cached.
- Authors keep one file to maintain and need no plugin package; strict
  third-party validators report the field like they report Dalo's existing
  fields.
- Approving a skill is not enough to run its binary; the separate approval
  stays visible in `approve skill`, `status`, and `doctor` output.
- Supporting a new download source, archive assets, or attestation is an
  additive declaration change, not a new file format.

## Evidence

Unit tests in `src/binary.rs` run against a loopback HTTP fixture and cover the
download and verification path: a matching digest stages a read-only file at
`binaries/<sha256>/<id>` and a second fetch makes no request; a tampered body fails
with `binary verification failed` and leaves nothing under `binaries/`; a
`Content-Length` above 256 MiB is refused before the body is read, and a streamed
body past the limit is refused as it is read; a redirect is followed only to an
allowed target, and one to a host outside GitHub is refused before it is
contacted; `expose` links the staged file with a relative target, replaces a stale
Dalo link, and refuses a real file or a foreign symlink without removing it.
Integration tests in `tests/cli.rs` run without network access: an undeclared host
writes nothing, a dry run plans only, pre-staged bytes approve offline and expose
`bin/<id>`, revocation keeps the bytes and reports `revoked`, tampered bytes fail
the audit with `binary_audit_failed`, `approve skill` names each pending binary,
and leftover download debris is reported as `binary_staging_debris`.

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

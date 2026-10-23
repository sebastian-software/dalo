# ADR 0013: Shared Version Update Client

Status: Accepted
Date: 2026-10-23

## Context

Dalo's passive update path previously owned GitHub lookup, cache cadence,
background-thread lifetime, and notice markers. A shared version service and
published Rust client now provide that behavior across tools. Different
installation channels can finish publication at different times.

## Decision

Dalo uses the published `app-tracking` Rust client and the Bunny version
service. The native CLI owns checks, including npm/npx launches. The shared
client owns transport limits, version comparison, cache/claims, failure
backoff, and notice throttling. Dalo owns eligibility, installation detection,
and upgrade guidance. It performs a bounded synchronous check after successful
commands; no update worker depends on surviving CLI exit.

Every installation uses `dalo/github/stable`. app-tracking reads the existing
CDN-cached oss-metrics document; oss-metrics owns GitHub release discovery.
Releases become eligible after one hour to allow downstream publication to
catch up. This is a shared release version, not a guarantee of package-manager
availability. Installation detection only selects upgrade guidance and does not
partition checks or cache state. The
[reference](../reference.md#update-notices) owns exact limits and cache layout.

Existing disabled/offline/CI/JSON policies remain effective. The update-only
client creates no telemetry identity, project hash, invocation count, or
reporting queue. Future telemetry remains a separate opt-in integration.

## Consequences

- A shared client release can improve caching and concurrency across tools.
- A version service failure remains advisory and cannot change command output
  or exit status. There is no parallel legacy GitHub check.
- The old advisory cache is left untouched and ignored; the new cache uses a
  versioned subdirectory and atomic process-safe state.
- Homebrew, Cargo, Nix, Debian, and unknown installations retain passive checks.
- The one-hour release delay reduces publication races; downstream outages can
  still require retrying an upgrade later.

Tracked in [app-tracking issue #4](https://github.com/sebastian-software/app-tracking/issues/4)
and [Dalo issue #1001](https://github.com/sebastian-software/dalo/issues/1001).

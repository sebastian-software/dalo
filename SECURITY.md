# Security Policy

Dalo clones Git repositories, reads skill metadata, writes lock/config files, and creates symlinks into agent skill folders. Security issues can affect local files, trusted automation, or agent behavior, so please report suspected vulnerabilities privately.

## Supported Versions

Security fixes are provided for the latest release on the default branch. Older
releases are not patched separately — upgrade to the latest version to receive a
fix.

## Reporting a Vulnerability

Report suspected vulnerabilities privately. Do not open a public issue, pull
request, or discussion for a vulnerability that has not been fixed yet.

Two private channels are available:

- **GitHub private vulnerability reporting** — open this repository's
  **Security** tab and choose **Report a vulnerability**.
- **Email** — security@sebastian-software.de.

Include a concise description, the affected version or commit, reproduction
steps, the impact you expect, and any suggested fix. Leave out credentials and
data you are not allowed to share.

## Response Expectations

Maintainers aim to:

- Acknowledge a private report within 7 days.
- Triage severity and affected versions within 14 days.
- Coordinate a fix and disclosure timeline with the reporter.
- Credit the reporter when desired and appropriate.

Timing may vary for low-impact reports or reports that require upstream dependency fixes.

## Scope

Examples of in-scope issues:

- Path traversal or unsafe source/skill ID handling.
- Overwriting unmanaged user files or foreign symlinks.
- Incorrect ownership tracking that deletes or rewrites user content.
- Unsafe parsing of `SKILL.md`, instruction packs, config, lock, state, approvals, or source-lock files.
- Git command invocation that allows argument injection or unexpected interactive behavior.
- Approval or trust logic that activates unapproved skills.
- Release or packaging issues that could ship the wrong artifact.

Examples usually out of scope:

- Reports without a concrete impact path.
- Vulnerabilities in a third-party skill repository unless Dalo mishandles it.
- Expected behavior from intentionally trusted local or team sources.
- Denial of service from manually corrupting local store files, unless it leads to unsafe mutation or privilege escalation.

## Security Model Notes

Dalo should fail closed when state is unsafe. Dirty sources, unsupported schema versions, malformed instruction blocks, store locks, and ambiguous target conflicts should block mutation or report actionable diagnostics.

Dalo-managed instruction blocks are delimited by explicit markers, and content outside managed blocks should be preserved. Dalo-owned target entries are tracked in state; unmanaged real entries must not be replaced unless the user explicitly requests an operation designed to do that, such as `adopt --replace`.

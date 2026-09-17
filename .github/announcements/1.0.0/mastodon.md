# Mastodon

One post, under 500 characters, plus an optional reply for the detail. The
fediverse audience cares about licensing, telemetry, and what the tool refuses
to do, so those are in the first post rather than a footnote. Add alt text if
you attach a screenshot.

## Post (460 characters)

<!-- post -->
Dalo 1.0 is out. It keeps your team's agent skills in Git and links one resolved, approved set into the folders your agents already read, so two people on the same team stop running different versions of the same skill.

Sources pinned to exact commits. A local preflight reads every skill, never runs one, and blocks the sync until a person approves.

No telemetry, no account, single binary. macOS and Linux. MIT or Apache-2.0.

https://dalo.sh/news/1-0.html

## Reply (optional, 426 characters)

<!-- post -->
Two things it deliberately does not do.

It does not make third-party instructions safe. The preflight is pattern based and the approval is a human decision, and the security page lists what still gets through, including prompt injection against your own agent.

It does not touch what you wrote. An unmanaged folder is reported, never replaced, and instruction packs render into a marked block inside your existing AGENTS.md.

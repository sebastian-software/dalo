# LinkedIn

Single post. Audience: engineers and team leads who already run agents at work.
Angle: the team problem first, the release second. Post as a text post with the
link in the body; no image is required.

---

Dalo 1.0 is out.

If your team runs Claude Code, Codex, or another agent, you already know the
failure mode. Everyone has skills in their own agent folder. Nobody knows whose
version is current. A skill that was reviewed once gets edited upstream and
nobody notices, because nothing asked.

Dalo keeps those skills in Git and links one resolved, approved set into the
folders your agents already read. Your agents learn nothing new. They keep
reading ~/.claude/skills or ~/.agents/skills. Dalo owns what is behind them:
sources, priorities, approvals, conflicts, drift, and it repairs them on the
next sync.

Three things it does that a plain install command does not:

A team repository carries a committed dalo.toml naming the team's own skills and
pinning the public catalogs everyone gets. A teammate adds the source, runs
sync, and has exactly what the manifest says, down to the commit.

Nothing reaches an agent folder unreviewed. A deterministic local preflight
reads every skill, never executes one, and blocks the sync on high or critical
findings until a person records an exception that is bound to the content. It
does not make third-party instructions safe, and the docs say exactly where that
line is.

Your hand-written files stay yours. An unmanaged folder is reported, not
replaced. Instruction packs render into a marked block inside your existing
AGENTS.md and leave the rest alone.

1.0 is the release where the surface stops moving: commands, flags, exit codes,
JSON shapes, and every file Dalo persists are fixed for the whole 1.x line, and
a store written by 0.6.0 still opens with no migration step. That part is a
test, not a promise: the suite replays stores written by four older released
binaries and asserts the resolved skill set is unchanged.

macOS and Linux, single binary, MIT or Apache-2.0, no telemetry.

Read the release: https://dalo.sh/news/1-0.html

#AI #DeveloperTools #Rust #OpenSource

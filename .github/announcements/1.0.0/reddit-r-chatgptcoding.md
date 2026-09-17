# r/ChatGPTCoding

This sub runs several agents side by side, Codex most of all, and switches
tools often. The question it asks is "do I have to redo my setup for every
agent, and how do I keep the same context across them". So the post is about
one set of skills across several agents, with drift and review as the reason
the set has to be resolved rather than copied.

It is not the r/ClaudeAI post with a word swapped: the lead is multi-agent, not
team-of-humans, and the team angle arrives second. Check the sub's
self-promotion rules and flair before posting.

## Title

<!-- title -->
Dalo 1.0: one Git-backed skill set that Codex, Claude Code, and the rest all read from

## Body

<!-- body -->
If you run more than one coding agent, you have the same text in several
places. A skill in `~/.codex`, a near copy in `~/.claude/skills`, another in
whatever you tried last month. Improve one and the others rot. Switch tools and
you rebuild the setup.

Dalo 1.0 handles that from the other side. Skills live in Git. Dalo resolves
one approved set and links it into the folders each agent already reads. No
plugin, no wrapper, no proxy: the agents are unchanged and keep reading their
own directories.

    dalo target detect          # read-only, names each installed agent's skill dir
    dalo target link codex
    dalo target link claude
    dalo source add company https://github.com/acme/agent-skills.git
    dalo sync

Codex, Claude Code, OpenClaw, Hermes, OpenCode, and a `generic` target for any
folder-based tool are supported in 1.0, and the docs record which version of
each agent the path was verified against, with the date. If your agent is not
in that list but reads a folder, `dalo target link generic <path>` covers it.

Why resolve instead of copy:

**Pinning.** Sources are Git repositories pinned to exact commits in a
lockfile. Two machines, or the same machine next month, get the same content.
Upstream changes are shown as drift you advance on purpose, not content that
quietly arrives.

**A review gate.** Before anything is linked, a deterministic local preflight
reads every skill. It never executes one. High and critical findings block the
sync until you record an exception, and that exception is bound to the content,
so an upstream edit blocks again. If you pull skills from public repositories,
this is the part that earns its keep.

**Your files survive.** A path Dalo does not own is reported as unmanaged and
left alone. Instruction packs render into a marked block inside your existing
`AGENTS.md` or `CLAUDE.md` and never touch what you wrote around it.

**Priorities, not guesswork.** Several sources with the same skill name is a
conflict Dalo resolves by declared priority and reports, instead of whichever
copy was written last.

The team case follows from the same mechanism: a committed `dalo.toml` in a
team repository names the skills and pins the catalogs, so a new colleague runs
two commands and has exactly what everyone else has.

What it is not: it is not a skill directory or a marketplace, there is nothing
to browse, and it does not make third-party instructions safe. It is macOS and
Linux only, with Windows through WSL. And for trying a single public skill
quickly, `npx skills add` is honestly less work; a comparison with that CLI and
with agentfiles is at https://dalo.sh/docs/comparison.html.

1.0 means the surface stops moving: commands, flags, exit codes, `--json`
shapes, and every persisted file are fixed for the whole 1.x line. Single
binary, MIT or Apache-2.0, no telemetry.

- Release notes: https://dalo.sh/news/1-0.html
- Docs: https://dalo.sh/docs/
- Repository: https://github.com/sebastian-software/dalo

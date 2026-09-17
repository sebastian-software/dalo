# Dalo compared with skills.sh and agentfiles

Three tools, three different jobs. Vercel's [skills.sh](https://skills.sh) and
its `skills` CLI make it fast to discover and install a public skill into
almost any agent. [agentfiles](https://github.com/Railly/agentfiles) gives you
a visual place in Obsidian to browse and edit the skills, commands, and rules
of many agents. Dalo manages the approved, reproducible skill set that a team
delivers to its agents.

This page is a snapshot from September 2026 (skills CLI v1.6.0, agentfiles
0.9.1, Dalo 0.15). All three projects move quickly; if something here is out
of date, please [open an issue](https://github.com/sebastian-software/dalo/issues).

## At a glance

| | Dalo | skills.sh (`skills` CLI) | agentfiles |
| --- | --- | --- | --- |
| Kind | CLI with a Rust library core | Node.js CLI plus public directory | Obsidian plugin (desktop) |
| Main job | Team-wide, reviewed skill delivery | Discover and install skills | Browse and edit agent files visually |
| Sources | Git repositories, catalogs, private local source | GitHub, GitLab, any Git URL, local paths, archives | skills.sh search, GitHub default branch |
| Delivery into agents | Symlinks from a Git-backed store | Canonical copy plus symlinks (or `--copy`) | Copies into each agent folder |
| Version pinning | Lockfiles with exact commits | Branch or tag plus content hash | Records the commit, updates to latest `HEAD` |
| Updates | Preview drift, then advance explicitly | `skills update` pulls latest content | "Update all" pulls latest `HEAD` |
| Existing folders | Unmanaged files are never overwritten | Target folder is replaced | Target folder is replaced after install |
| Review and trust | Security preflight, audits, per-skill approval | Audit pages on skills.sh | None built in |
| Beyond skills | Instruction packs, portable plugins, approved tools and hooks | Skills only | Views commands, agents, and rules; installs skills only |
| Agent coverage | 5 verified targets plus any folder | About 80 agents | 17 tools |
| Platforms | macOS, Linux | macOS, Linux, Windows | Obsidian desktop |
| Telemetry | None | Anonymous, on by default, opt-out | None, according to its README |
| License | MIT or Apache-2.0 | MIT | MIT |

## skills.sh and the `skills` CLI

[skills.sh](https://skills.sh) is the public directory for the Agent Skills
ecosystem, with install leaderboards, topics, and audit pages. The
[`skills` CLI](https://github.com/vercel-labs/skills) (`npx skills add
owner/repo`) installs `SKILL.md` folders from GitHub, GitLab, any Git URL, or a
local path into about 80 agents, in project or global scope.

What it does well:

- **Reach.** Nearly every agent has a known project and global path, and it
  also runs on Windows.
- **Discovery.** Search, trending lists, and install counts make it easy to
  find a skill in the first place.
- **Low friction.** One `npx` command, no store to set up. `skills use` even
  runs a skill without installing it.
- **Symlinked delivery.** By default it keeps one canonical copy in
  `.agents/skills` and links it into each agent folder, much like Dalo.

Where the model differs from Dalo:

- **Pinning.** `skills-lock.json` records a branch or tag and a content hash,
  not a commit. `skills update` fetches the latest content for that ref, so two
  machines can resolve different code from the same lockfile.
- **Existing content.** Installing clears the target directory and replaces an
  existing folder or link at the agent path. Dalo reports such a path as
  unmanaged and leaves it alone.
- **Trust.** Nothing asks you to approve a skill before it lands in an agent
  folder. Dalo requires an explicit selection and approval, and blocks
  high-severity audit findings during sync.
- **Telemetry.** The CLI sends anonymous usage data by default; set
  `DISABLE_TELEMETRY=1` or `DO_NOT_TRACK=1` to turn it off.

## agentfiles

[agentfiles](https://github.com/Railly/agentfiles) is an Obsidian plugin that
turns a vault into a manager for the files of 17 AI tools: Claude Code, Cursor,
Windsurf, Codex, Copilot, and more. It also ships a Claude Code conversation
browser, a usage dashboard, and an early VS Code extension.

What it does well:

- **A visual home.** Browse, search, create, and edit skills, commands,
  agents, and rules side by side instead of in hidden dot folders.
- **Breadth of file types.** It shows more than skills, including memories and
  rules, across many tools at once.
- **Marketplace on top of skills.sh.** It searches skills.sh and installs from
  GitHub with size limits, path-traversal checks, and rollback on failure.

Where the model differs from Dalo:

- **Copies, not links.** Installs write a copy into `~/.agents/skills` and into
  each chosen agent folder, so copies can drift apart.
- **Latest, not pinned.** It records the installed commit in the `skills` lock
  format, but updating always resolves the default branch's current `HEAD`.
- **Personal scope.** There is no team manifest, source priority, or approval
  step; it is built for one person's vault and machine.
- **Replaces existing folders.** An existing target folder is moved aside and
  deleted once the new install succeeds.

## When to use which

- **Use skills.sh** to find skills and to try one quickly, especially on
  Windows or in an agent Dalo does not target yet.
- **Use agentfiles** if you live in Obsidian and want to read and edit what
  all your agents are loading, in one visual place.
- **Use Dalo** when skills are team knowledge: several people or machines need
  the same reviewed set, upstream changes must not arrive silently, and local
  experiments and hand-written agent files must stay untouched.

They also combine. A repository you found on skills.sh is a normal Git
repository, so Dalo can pin it as a catalog and deliver only the skills you
select and approve:

```sh
dalo source add-catalog public https://github.com/vercel-labs/agent-skills.git
dalo source inspect public
```

## Sources

- skills CLI: [README](https://github.com/vercel-labs/skills#readme) and the
  `installer`, `local-lock`, `skill-lock`, `update`, and `telemetry` modules in
  [vercel-labs/skills](https://github.com/vercel-labs/skills), v1.6.0
- skills.sh: [directory and leaderboard](https://skills.sh)
- agentfiles: [README](https://github.com/Railly/agentfiles#readme), `manifest.json`,
  and `src/marketplace.ts` in [Railly/agentfiles](https://github.com/Railly/agentfiles), 0.9.1
- Dalo: [README](../README.md), [command reference](reference.md), and
  [agent integration](agents.md)

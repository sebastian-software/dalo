# Hacker News

A Show HN with the site as the URL and a first comment posted immediately after
submitting. HN readers punish adjectives and reward limits, so the comment
leads with the mechanism, names the alternatives, and says what Dalo is not.

## URL

https://dalo.sh/news/1-0.html

## Title (77 characters)

<!-- title -->
Show HN: Dalo – team agent skills in Git, reviewed before they reach an agent

Alternatives, if the title above reads badly in the list:

- `Show HN: Dalo 1.0 – one approved set of agent skills for a whole team` (69)
- `Show HN: Dalo – version your team's AI agent skills like code` (61)

## First comment

<!-- comment -->
I built this at a consultancy where several people run Claude Code and Codex on
the same projects. The recurring problem was not writing skills, it was that
everyone's agent folder held a slightly different copy of the same one, and
nobody could say which copy had been reviewed.

Dalo keeps the skills in Git and links one resolved set into the folders agents
already read. The agents are unmodified: they keep reading `~/.claude/skills`
or `~/.agents/skills`. What is behind those folders is what Dalo owns.

How it works, concretely:

- Sources are Git repositories, pinned in a lockfile to exact commits. A team
  repository carries a committed `dalo.toml` that names the team's own skills
  and pins the public catalogs everyone gets, so the manifest, not the
  individual machine, decides URL, revision, priority, and selection.
- Delivery is symlinks from a local store. A path that Dalo does not own is
  reported as unmanaged and left alone rather than replaced, which is the part
  I care most about: hand-written agent files survive a sync.
- Before anything is linked, a deterministic local preflight reads every skill.
  It never executes one. High and critical findings block the sync until a
  person records an exception, and that exception is bound to the content, so
  an upstream edit re-blocks. Approvals are source qualified, so approving
  `company:pr-review` never silently approves someone else's `pr-review`.
- Plugins can also carry executable tools and hook contracts. Those are
  approved separately from skills, by exact contract hash. When a package has
  to build a provider-specific artifact, the generator runs under Landlock on
  Linux or Seatbelt on macOS, fail closed: no sandbox, no execution.

What it is not. It is not a skill directory or a discovery tool; there is no
index to browse and nothing is ranked. It does not make third-party
instructions safe, and `docs/security.md` has a long section on exactly what
still gets through, including prompt injection against your own agent, a
compromised upstream account, and anything a skill triggers at runtime. There
is no native Windows build in 1.x, only WSL. The Rust library is not a semver
contract; the CLI, its exit codes, and its `--json` output are.

Nearest neighbours: Vercel's `skills` CLI reaches far more agents and is much
lower friction for trying one skill, and agentfiles is a nicer place to read
and edit agent files if you live in Obsidian. Both replace an existing target
folder and both resolve the latest content for a ref rather than a commit,
which is the tradeoff I did not want for a team.
https://dalo.sh/docs/comparison.html has the details and the versions it was
checked against.

1.0 means the surface stops moving: commands, flags, the five exit codes,
`--json` shapes, and every persisted file are fixed for the 1.x line, and a
store written by 0.6.0 opens with no migration command. That last part is a
test rather than a claim; the suite restores stores written by four older
released binaries and asserts the resolved set is unchanged and the second sync
is a no-op.

Rust, single binary, no daemon, needs `git` on your PATH. MIT or Apache-2.0.
No telemetry. Happy to answer anything.

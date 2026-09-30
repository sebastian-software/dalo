# Agent Target Integration

This page is the single support matrix for Dalo's agent targets. Portable
canonical agent packages are documented in the
[command reference](reference.md#dalo-agent-list---check-dalo-agent-show-sourcename).

Dalo's built-in targets are directory-based. Each target links the resolved skill set
into the folder that agent already reads: Dalo writes one symlink per skill into
the target directory and never touches the directory's unmanaged entries. An
agent therefore only works with Dalo if it follows a symlinked skill directory.

Run this first:

```sh
dalo init
dalo target detect
```

## Support matrix

| Target ID | Agent | Support | Verified version | Verified on | User-level path | Project-level path | Symlinked skill directory |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `codex` | Codex CLI | supported | `codex-cli 0.154.0` | 2026-09-17 | `~/.agents/skills` | `.agents/skills`, from the working directory up to the repository root | yes, observed |
| `claude` | Claude Code | supported | `2.1.235` | 2026-09-17 | `~/.claude/skills` | `.claude/skills` | yes, observed |
| `cursor` | Cursor Agent | supported | `3.13.25` | 2026-09-28 | `~/.cursor/skills` | `.cursor/skills` | yes, invoked |
| `openclaw` | OpenClaw | supported | `2026.9.4` | 2026-09-17 | `~/.agents/skills` | `<workspace>/skills`, `<workspace>/.agents/skills` | yes, observed |
| `hermes` | Hermes | supported | `v2026.9.14` (latest release) | 2026-09-17 | `~/.hermes/skills` | `.hermes/skills`, `.agents/skills`, after `hermes skills trust` | not verified on this date |
| `opencode` | OpenCode | supported | `1.18.31` | 2026-09-17 | `~/.config/opencode/skills` | `.opencode/skills` | yes, observed |
| `generic` | any folder-based agent | supported | — | — | the path you pass | the path you pass | depends on the agent |

Cursor has a built-in target; see [Cursor](#cursor) for verification details
and project-level setup.

Codex through OpenCode were verified on 2026-09-17 on macOS, in a throwaway
`HOME` holding a single probe skill whose directory is a symlink to a directory
elsewhere — the exact shape `dalo sync` produces. Cursor was verified separately
on 2026-09-28; see its section below.

## Project-scoped folders

There are two ways to deliver skills into a repository's own agent folders.

**A project installation** is the reproducible one. The repository commits a
`dalo-project.toml` that pins its sources, and `dalo install` restores them into
an isolated `.dalo/` store and the project folders of the declared agents, in
every clone. See [Project installations](projects.md). Project scope supports
the `claude`, `codex`, `opencode`, `hermes`, and `openclaw` targets.

**Redirecting a target** reuses your existing store and is a per-machine
recipe. A target in that store links a **user-level** folder by default. Every
agent in the matrix also reads a project-level folder — `.claude/skills` for
Claude Code, `.agents/skills` for Codex and OpenClaw, `.opencode/skills` for
OpenCode, `.hermes/skills` for Hermes — and a target can be pointed at one of
those paths inside a repository:

```sh
dalo target link claude /path/to/repo/.claude/skills
dalo target link generic /path/to/repo/.agents/skills
dalo sync
ls -la /path/to/repo/.claude/skills
```

Both directories then hold one symlink per active skill, exactly as a user-level
target does.

Read the caveats before you rely on this. A target ID holds exactly one path, so
covering two folders in one repository consumes two target IDs and takes those
IDs away from your home directory; a second repository has no ID left. Targets
live in the store, not in the repository, so a teammate or a second machine has
to run the same commands by hand. The symlinks point at absolute store paths and
must not be committed:

```gitignore
/.claude/skills/
/.agents/skills/
```

`dalo target detect` prints one informational line when the working directory
holds a project-scoped agent folder that no linked target covers. It does not
link anything.

Project installations shipped in Dalo 1.1 through
[#851](https://github.com/sebastian-software/dalo/issues/851). The
[FAQ entry](troubleshooting.md#can-dalo-manage-my-repositorys-claudeskills)
compares both routes and has the full redirect recipe.

## Codex

Default skill path:

```text
~/.agents/skills
```

Link and verify:

```sh
dalo target link codex
dalo status
dalo sync
ls -la ~/.agents/skills
```

How this row was verified: ran `codex debug prompt-input` with `HOME` pointed at
the throwaway home and the working directory set to a scratch project. The
rendered prompt listed the skill roots `r0 = $HOME/.agents/skills`,
`r1 = $CODEX_HOME/skills/.system` and `r2 = $CWD/.agents/skills`, and both
symlinked probe skills appeared under its `Available skills` heading. The
[Codex skills documentation](https://developers.openai.com/codex/skills) states
the same user, repository and admin locations and says Codex follows the symlink
target when scanning them.

## Claude Code

Default skill path:

```text
~/.claude/skills
```

Link and verify:

```sh
dalo target link claude
dalo status
dalo sync
ls -la ~/.claude/skills
```

How this row was verified: ran `claude --debug --debug-file <path> -p` with
`HOME` pointed at the throwaway home, an invalid API key and an unreachable API
base URL, so the run never reached the model. The debug log recorded
`Loading skills from: managed=/Library/Application Support/ClaudeCode/.claude/skills, user=<HOME>/.claude/skills, project=[<cwd>/.claude/skills]`
followed by `Loaded 2 unique skills (2 unconditional, 0 conditional, managed: 0,
user: 1, project: 1, additional: 0, legacy commands: 0)` — both of them
symlinked directories.

## OpenClaw

Default skill path:

```text
~/.agents/skills
```

Link and verify:

```sh
dalo target link openclaw
dalo status
dalo sync
ls -la ~/.agents/skills
```

Codex and OpenClaw can share the same physical directory. Dalo de-duplicates physical target paths during materialization.

How this row was verified: installed `openclaw@2026.9.4` from npm into a
temporary prefix and ran `openclaw skills list --json` with `HOME` pointed at the
throwaway home. The symlinked probe skill was listed with
`"source": "agents-skills-personal"`, `"eligible": true` and
`"modelVisible": true`. The
[OpenClaw skills documentation](https://docs.openclaw.ai/tools/skills) documents
`~/.agents/skills` as the personal skill root for the default state directory
and `<workspace>/skills` plus `<workspace>/.agents/skills` as the project roots.

## Hermes

Default skill path:

```text
~/.hermes/skills
```

Link and verify:

```sh
dalo target link hermes
dalo status
dalo sync
ls -la ~/.hermes/skills
```

How this row was verified: not verified on this date. Hermes ships no
non-interactive command that lists skills, and listing them from a session
requires model credentials. The latest release is `v2026.9.14` (2026-09-14). The
user-level path `~/.hermes/skills` and the project-level paths come from the
official
[Hermes skills documentation](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills).
Hermes also scans additional directories listed under `skills.external_dirs` in
`~/.hermes/config.yaml`, which is the supported way to point it at a shared
`~/.agents/skills`.

## OpenCode

Default skill path:

```text
~/.config/opencode/skills
```

Link and verify:

```sh
dalo target link opencode
dalo status
dalo sync
ls -la ~/.config/opencode/skills
```

OpenCode also reads `~/.claude/skills` and `~/.agents/skills` unless
`OPENCODE_DISABLE_EXTERNAL_SKILLS=1` is set, so a linked `claude` or `codex`
target already reaches it. Link `opencode` when you want the resolved set in
OpenCode's own directory.

How this row was verified: ran `opencode --pure debug skill` with `HOME` pointed
at the throwaway home. Both probe skills were listed with their symlinked
locations, `<HOME>/.config/opencode/skills/dalo-probe/SKILL.md` and
`<cwd>/.opencode/skills/dalo-probe-project/SKILL.md`. The
[OpenCode skills documentation](https://opencode.ai/docs/skills/) documents
`~/.config/opencode/skills/<name>/SKILL.md` as the global location.

## Cursor

Default skill path:

```text
~/.cursor/skills
```

Link user-level or project-level skills:

```sh
# User-level
dalo target link cursor
dalo sync
ls -la ~/.cursor/skills

# Project-level, by redirecting the target of your existing store
dalo target link cursor /path/to/repo/.cursor/skills
dalo sync
```

A [project installation](projects.md) has no `cursor` target yet. Cursor's
documentation also lists a project's `.agents/skills` and `.claude/skills` as
skill roots, so a project that declares the `codex` or `claude` target should
reach it; that route was not part of the verification below.

Each target ID records one directory. Cursor also discovers `~/.agents/skills`
and `.agents/skills`; a `codex` target already materializes the user-level
shared directory for Cursor.

The [Cursor skills documentation](https://cursor.com/docs/skills) lists
`~/.cursor/skills/` and `~/.agents/skills/` as user-level roots, and
`.cursor/skills/`, `.agents/skills/`, `.claude/skills/`, and `.codex/skills/` as
project roots. On 2026-09-28, Cursor 3.13.25 on macOS arm64 discovered and
invoked Dalo-managed symlinked skills from both `~/.cursor/skills/` and a
project's `.cursor/skills/`; each invocation returned the marker in its test
skill.

Dalo blocks a sync when a real unmanaged directory already occupies the skill's
target slot and leaves that directory unchanged. Cursor reads project
`AGENTS.md` files as instructions; Dalo can add instruction packs to an explicit
file with `dalo instructions enable <pack-ref> ./AGENTS.md`. There is no native
Cursor instruction-file target mapping, so use an explicit file. Dalo preserves
user-authored content outside its managed block. See the
[Cursor rules documentation](https://cursor.com/docs/rules) and the
[instruction reference](reference.md).

## Any other folder-based agent

```sh
dalo target link generic /path/to/agent/skills
dalo sync
```

Open an issue with the skill path and a short verification transcript if you can
confirm a built-in target for another agent.

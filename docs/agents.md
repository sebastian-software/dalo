# Agent Target Integration

This page is the single support matrix for Dalo's agent targets. Portable
canonical agent packages are documented in the
[command reference](reference.md#dalo-agent-listshow-sourcename).

Dalo's V1 targets are directory-based. Each target links the resolved skill set
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
| `openclaw` | OpenClaw | supported | `2026.9.4` | 2026-09-17 | `~/.agents/skills` | `<workspace>/skills`, `<workspace>/.agents/skills` | yes, observed |
| `hermes` | Hermes | supported | `v2026.9.14` (latest release) | 2026-09-17 | `~/.hermes/skills` | `.hermes/skills`, `.agents/skills`, after `hermes skills trust` | not verified on this date |
| `opencode` | OpenCode | supported | `1.18.31` | 2026-09-17 | `~/.config/opencode/skills` | `.opencode/skills` | yes, observed |
| `generic` | any folder-based agent | supported | — | — | the path you pass | the path you pass | depends on the agent |

Cursor has no built-in target ID; see [Cursor](#cursor) for the reason and the
command that covers it.

Every observation below was made on 2026-09-17 on macOS, in a throwaway `HOME`
holding a single probe skill whose directory is a symlink to a directory
elsewhere — the exact shape `dalo sync` produces.

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

Cursor has no built-in target ID. Point a generic target at the folder Cursor
reads:

```sh
dalo target link generic ~/.cursor/skills
dalo sync
ls -la ~/.cursor/skills
```

Cursor also loads `~/.agents/skills`, so a linked `codex` or `openclaw` target
already reaches it.

Why there is no `cursor` target: discovery could not be verified on this date.
The installed Cursor CLI (`cursor-agent 2025.09.18-7ae6800`) reports
`Not logged in` and offers no offline command that lists skills, so a symlinked
skill directory could not be observed without signing in. The
[Cursor skills documentation](https://cursor.com/docs/context/skills) documents
`~/.cursor/skills/` and `~/.agents/skills/` as the user-level roots and
`.cursor/skills/`, `.agents/skills/`, `.claude/skills/` and `.codex/skills/` as
project roots. Dalo 1.0 does not ship a target for an unverified promise.

## Any other folder-based agent

```sh
dalo target link generic /path/to/agent/skills
dalo sync
```

Open an issue with the skill path and a short verification transcript if you can
confirm a built-in target for another agent.

# Agent Integration

Dalo's V1 targets are directory-based. Each target links the resolved skill set into the folder that agent already reads.

Run this first:

```sh
dalo init
dalo target detect
```

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

## Cursor and OpenCode

Cursor and OpenCode have experimental built-in target IDs, but neither has a
verified default path. Until their behavior is verified, use a generic target
after you know the folder the agent reads:

```sh
dalo target link generic /path/to/agent/skills
dalo sync
```

You may instead use `dalo target link cursor <path>` or
`dalo target link opencode <path>` to record the experimental target ID. All
three forms require an explicit path; `generic` is recommended until the adapter
is verified.

Open an issue with the expected skill path and a short verification transcript if you can confirm either target.

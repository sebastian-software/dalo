# Five Minutes to the First Skill

At the end you have one team skill, linked into the agent you use.

## Through your agent

> Install Dalo, the agent skill manager.

The agent installs the binary, connects its skill folder, and delivers the
bundled assistant. Ask it "Add my team's skill repository," and you are done.
[Use Dalo through your agent](assistant.md) covers what comes next.

## In the terminal

The output below is real. The repository in step 3 is a placeholder for yours.

**1. Run `dalo`.** The first run creates `~/.dalo` and prints the summary with
the next step.

```sh
dalo
```

```text
assistant: prepared the bundled Dalo skill in ~/.dalo/local/skills/dalo
assistant: next: dalo target detect
Dalo
  store: ~/.dalo
  initialized: yes
  linked targets: 0
  sources: 1
  active skills: 1
  pending approvals: 0

Next: dalo target detect
  Link a target so Dalo knows where to link skills.
```

**2. Link the agent you use.** This example uses Claude Code.

```sh
dalo target link claude
```

```text
linked target claude -> ~/.claude/skills
canonical: ~/.claude/skills
assistant: delivered to ~/.claude/skills/dalo; reload your agent's skills if needed, then select dalo
```

**3. Add your team's skill repository.**

```sh
dalo source add company git@github.com:acme/agent-skills.git
```

```text
added source company (2 skills checked, no findings)
next: dalo sync
```

**4. Sync.** Dalo resolves the approved skills and links them into every target.

```sh
dalo sync
```

```text
dalo store: ~/.dalo
target[claude]: ~/.claude/skills
existing noop       target[claude]:/dalo -> store:/local/skills/dalo
applied  create     target[claude]:/incident-review -> store:/sources/company/checkout/skills/incident-review
applied  create     target[claude]:/release-notes -> store:/sources/company/checkout/skills/release-notes
synced: 3 skills across 1 target (2 created, 1 unchanged)
security preflight: deterministic checks only
```

Check the result once:

```sh
dalo status
```

```text
dalo store: ~/.dalo
sources:
  local        local priority=0    skills=1
  company      team priority=10   skills=2
targets:
  claude       linked  ~/.claude/skills
active skills:
  dalo -> local:dalo
  incident-review -> company:incident-review
  release-notes -> company:release-notes
```

Your agent now reads those skills from the folder it already uses.

## Eight words you'll meet

- **Skill**: a folder with a `SKILL.md` file that tells an agent how to do a task.
- **Source**: a Git repository or local folder that Dalo reads skills from.
- **Catalog**: a public source with many skills. You select the ones you want.
- **Target**: an agent's skill folder, such as `~/.claude/skills`.
- **Approve**: trust one catalog skill, so that sync may link it.
- **Sync**: fetch updates, check them, and link the approved set into each target.
- **Adopt**: copy a skill an agent wrote into its own folder into your local source.
- **Conflict**: a folder you own already sits where Dalo wants a link.

## Where next

- [Getting started](getting-started.md): the full command-line walkthrough.
- [Team repository guide](team.md): publish the source your teammates add.
- [Project installations](projects.md): pin skills per repository.
- [Troubleshooting](troubleshooting.md): each status word and diagnostic, with the command that clears it.

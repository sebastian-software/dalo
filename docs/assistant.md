# Use Dalo through your agent

The Dalo skill lets you manage your skills by talking to the agent you already
use. Ask it to inspect your setup, bring existing skills under Dalo, preserve
your own changes, or update an existing installation. The agent runs Dalo and
explains the result; you do not need to remember its commands.

The skill lives in [`skills/dalo`](../skills/dalo/SKILL.md). It contains
instructions and supporting references, with no separate model, API key, server,
or background service. The agent needs filesystem access and permission to run
commands for changes. It can inspect existing folders before Dalo is installed.

## Start with an ordinary request

Tell your local coding agent: **“Install the Dalo skill manager.”** The official
website and repository point to the [agent installation guide](../site/install.md),
which takes the agent through installing the binary and making the assistant
available. You do not need a Dalo skill first, a special prompt, or a second
skill manager. The agent needs web or repository access to find the instructions
and local command access to perform the installation.

Discovery depends on the host's tools; if it cannot identify the project, give
it [dalo.sh](https://dalo.sh). The site's [llms.txt](https://dalo.sh/llms.txt)
is an additional documentation index. The same installation path is linked from
normal HTML and the README, so it does not depend on automatic llms.txt support.

## Bundled installation

On an ordinary interactive invocation, including bare `dalo` and after `dalo
init`, Dalo checks the assistant locally. If it is missing or differs from the
running binary's bundle, Dalo offers to install or update it with a `[y/N]`
question. Only `y` or `yes` applies the change; Enter, no, and end of input keep
it unchanged. With no store yet, the question explicitly includes creating the
store. Declining leaves the offer available on the next invocation. A current
bundle needs no question.

The check uses the installed binary's contents, not the latest internet release.
Modified bundles and discovered external installations receive a diagnostic
instead of an overwrite offer. The check also distinguishes local installation
from delivery: a current local skill can still need a target link or sync.
Accepting an offer prepares the local skill; it never silently syncs the whole
store or selects an agent for you. Existing links to an updated bundle see it
immediately.

JSON, redirected input/output, dry runs, CI, automation checks, and background
hook/tool/scheduler commands never ask questions. Set `DALO_ASSISTANT_CHECK=never`
to disable the automatic terminal offer. Agents can read
`dalo assistant status --json` (also included as `assistant` in `status --json`)
and ask in their own conversation. The skill checks this on each new invocation
and does not repeat a declined offer for every command in the same task.

Every binary built with assistant support includes the complete skill. After
installing Dalo, a fresh setup follows these steps (Codex is an example):

```sh
dalo init
dalo target detect
dalo target link codex
dalo assistant install
dalo --dry-run sync
dalo sync --check
```

For an existing setup, keep its store and target paths and inspect it first.
`assistant install` installs the bundled skill into `local/skills/dalo` without
network access, a Git commit, target changes, or a full sync. The subsequent
sync uses ordinary Dalo audits and ownership checks. Review its whole-store
preview: existing team sources may refresh, and all configured targets receive
active skills. Other skills are not adopted as part of installation.

Run `dalo assistant install` again after upgrading the binary to update an
unmodified bundle. Its versioned receipt protects local changes, additional
files, and foreign symlinks from replacement. Existing target links expose an
updated bundle immediately. A different or customized local `dalo` skill must
be preserved and deliberately moved before that slot can hold a bundle.

A foreign assistant in an agent folder also remains untouched. The assistant
can help plan a handover, but the installer does not take ownership of it. Once
the files are delivered, the agent can read `dalo/SKILL.md` to continue the same
conversation. If native discovery is delayed, reload skills or start a new
session as required by that host.

## Other installation routes

On a release without `dalo assistant install`, upgrade through your existing
installation channel or install the complete [`skills/dalo`](../skills/dalo/SKILL.md)
folder through your agent's skill installer. Keep `references` and `agents`
alongside `SKILL.md`. For example, with the
[skills CLI](https://github.com/vercel-labs/skills):

```sh
npx skills add sebastian-software/dalo --skill dalo --agent codex --global
```

To try unpublished work from this repository checkout:

```sh
npx skills add ./skills/dalo --skill dalo --agent codex --global
```

For Claude Code, use `--agent claude-code`. This route installs the skill first;
ask it to set up Dalo and it can install the missing binary. Installing the skill
alone does not migrate existing folders. Keep its installer ownership explicit
if you later switch to the bundled route.

## Talk to Dalo

Select the Dalo skill in your agent, or use its skill invocation syntax, such as
`$dalo` in Codex. You can then ask in your own language:

- “Dalo, what is installed and what needs attention?”
- “I used skills.sh before. Move these skills to Dalo and keep my changes.”
- “Bring my own skills under version control without sharing them publicly.”
- “I installed Dalo months ago. Get this setup working again.”
- “Update my skills and explain anything that needs a decision.”
- “Make the same skills available in Claude Code.”

A bare invocation starts with a local assessment and a suggested next step. It
does not install software or change your setup. A concrete request continues
through the relevant work, asking about choices that cannot be inferred, such
as two different versions of a skill with the same name.

## What the assistant accounts for

It combines Dalo's reports with inspection of relevant existing skill folders.
This includes folders not linked to Dalo, shared symlinks, project-only skills,
and available installer metadata. Missing permissions and uncertain provenance
remain visible instead of being treated as an empty installation.

For migration, the assistant distinguishes a preserved local snapshot from a
skill reconnected to an upstream repository. A snapshot keeps your content but
does not receive upstream updates. Installer metadata helps identify a source;
it does not prove that your local copy is unchanged or grant Dalo approval.

Real skill directories can use Dalo's audited adoption flow. Foreign symlinks
need an explicit handover with preserved content and recovery paths. The
assistant reports anything that remains managed by another installer, including
old lock records that might still cause that installer to update migrated paths.
Project-only skills stay a separate scope; moving them to a user-level target
would make them available in other projects too.

The assistant uses the capabilities of the installed Dalo version. It does not
add a native import command, first-class project profiles, or automatic foreign
lockfile conversion. It cannot guarantee that every unknown installer layout
can be migrated automatically. Dalo's own protections still enforce approvals,
dirty-source handling, and ownership during CLI operations; any necessary manual
handover is prepared and verified separately.

See [Getting started](getting-started.md) for the command-line path and
[Troubleshooting](troubleshooting.md) for diagnostic details.

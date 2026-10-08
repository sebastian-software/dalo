# Use Dalo through your agent

Talking to your agent is the recommended way to install and use Dalo. Describe
the result you want: add your team's skills, check for updates, connect another
agent, or get an old setup working again. Your agent inspects the installation,
runs the right Dalo commands, and explains what changed. You do not need to
learn the CLI or interpret its reports yourself. Dalo's approval, audit, and
ownership checks apply to the operations your agent runs through Dalo.

The skill lives in [`skills/dalo`](../skills/dalo/SKILL.md). It contains
instructions and supporting references, with no separate model, API key, server,
or background service. The agent needs filesystem access and permission to run
commands for changes. It can inspect existing folders before Dalo is installed.

## Start with an ordinary request

Paste this into your local coding agent:

> Install Dalo, the agent skill manager.

The agent follows the [installation guide](../site/install.md) to install the
binary, connect the intended skill folder, deliver the bundled assistant, and
verify the result. No Dalo skill is needed first. The agent needs web or
repository access to find the instructions and local command access to perform
the installation.

If your agent cannot find the official project, include the guide directly:

> Install Dalo from https://dalo.sh/install.md and set it up for this agent.

Discovery depends on the host's tools. The site's [llms.txt](https://dalo.sh/llms.txt)
is an additional documentation index. The same installation path is linked from
normal HTML and the README, so it does not depend on automatic llms.txt support.

## Keep using Dalo through conversation

Installation is the first request. The same agent can manage your skills from
then on. Select the Dalo skill in your agent, or use its skill invocation
syntax, such as `$dalo` in Codex. Ask in your own language:

| What you want | What to tell your agent |
| --- | --- |
| Understand your setup | “Dalo, what skills are installed and what needs attention?” |
| Add your team's skills | “Add our skill repository at this URL and make its skills available here.” |
| Review updates | “Update my skills and show me what changed.” |
| Automate team updates | “Update my global team setup each morning and tell me when something changes or needs attention. Keep my personal catalog pins.” |
| Connect another agent | “Make the same skills available in Claude Code.” |
| Preserve existing work | “I used skills.sh before. Move these skills to Dalo and keep my changes.” |
| Keep your own skills private | “Bring my own skills under version control without sharing them publicly.” |
| Recover an old setup | “I installed Dalo months ago. Get this setup working again.” |

You supply the goal and decide which sources and changes to trust. The agent
checks the actual folders, previews changes, and turns Dalo's reports into an
explanation and next step. It can account for the store and target paths already
in use, local edits, and skills installed by another tool.

A bare invocation starts with a local assessment and a suggested next step. It
does not install software or change your setup. A concrete request continues
through the relevant work, asking about choices that cannot be inferred, such
as two different versions of a skill with the same name.

## Automate updates

Ask your agent or host to run updates when you need them: on demand, at session
start where supported, or on a schedule you choose. The automation owns the
timing, retries, and notifications. Dalo performs the requested update and
reports its result through the CLI.

Choose the scope and update policy explicitly. A global team sync refreshes
tracking sources and follows catalog pins declared by the team; personal catalog
pins change only in a separately requested update. In a project, installation
restores the checked-in `dalo-project.toml`, while `project update` authors a new
pin. A normal Git pull does not run Dalo's installation or synchronization step.
If the declaration sets `approval = "declaration"`, the reviewed declaration
approves its selection, so the agent does not ask you to approve each skill in
every clone or worktree; otherwise each project store keeps its own local
approvals. See [Approval modes](projects.md#approval-modes).

For an initialized store, an automation can run:

```sh
dalo --store <store-path> --json sync --check
```

`--check` still applies the sync and exits nonzero when the result needs review.
It is not a read-only upstream check. A project automation instead uses
`dalo --project <project-path> --json install` to restore the current definition.
The [CI guide](ci.md) describes JSON reports, exit codes, and read-only catalog
drift checks.

The agent reads the result and uses its own notification channel for meaningful
changes or required decisions. Pending approvals, security findings, conflicts,
and local edits need attention; an unattended run does not grant new trust or
discard work. A request for recurring updates does not select a cadence or
notification policy for you.

Existing OS-native [autosync commands](reference.md#dalo-autosync-installstatusuninstall)
remain supported in Dalo 1.x. Agent-managed update workflows use the host's
automation facilities; desktop, webhook, and mail notifications are handled
there.

## Dalo's checks still apply in conversation

Your agent turns the request into a plan and explains the decisions. When it
runs Dalo, the same approval, audit, and ownership checks apply as in the
terminal. Catalog skills need approval, sync preserves unmanaged files and
reports unresolved conflicts, and dirty source checkouts block refresh so local
edits are preserved.

If a check blocks an operation, the agent can explain the finding and help you
choose what to do next. You keep the decisions about sources, skills, and risk;
the agent handles command syntax and reports. The [security overview](security.md)
describes these protections and their limits. They apply to Dalo operations;
the assistant is not a sandbox for everything your agent can do.

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

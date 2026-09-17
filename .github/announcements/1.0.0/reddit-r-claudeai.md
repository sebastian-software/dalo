# r/ClaudeAI

The question this sub actually asks is "how do I share skills with my team
without everyone's `~/.claude/skills` going its own way, and how do I know what
is in there". So the post is about drift and trust, it is written from the
Claude Code folder outwards, and it never claims Dalo changes how Claude works.

Check the subreddit rules for self-promotion and flair before posting; if a
"Project" or "Showcase" flair exists, use it. Answer comments for the first few
hours.

## Title

<!-- title -->
Dalo 1.0: keep your team's Claude Code skills in Git, with a review gate before anything lands in ~/.claude/skills

## Body

<!-- body -->
If you are the only person using Claude Code, `~/.claude/skills` is fine as it
is. This is for the case where three or four of you use it on the same
codebase.

What went wrong for us: everyone had a copy of the same skill, slightly
different. Someone improved one and it never reached the others. A skill copied
from a public repository got updated upstream and nobody noticed, because
nothing asked. And a new colleague's first week included "ask around for the
good skills".

Dalo 1.0 is the tool I ended up writing for that. It keeps skills in Git and
links one resolved, approved set into the folders your agents already read.
Claude Code is not modified and learns nothing new: it still reads
`~/.claude/skills`. Dalo owns what is behind that folder.

The parts that matter for a team:

**One manifest decides.** A team repository holds a committed `dalo.toml` with
your team's skills and the public catalogs everyone gets, pinned to exact
commits. A teammate runs two commands and has exactly that set, not roughly
that set.

    dalo target link claude
    dalo source add company https://github.com/acme/agent-skills.git
    dalo sync

**Nothing lands unreviewed.** Before a skill is linked, a deterministic local
preflight reads it. It never executes it. High and critical findings block the
sync until a person records an exception, and the exception is bound to the
content, so if upstream edits the skill later it blocks again on the next sync.
Approvals are source qualified: approving `company:pr-review` does not approve
someone else's `pr-review`.

**Your own files are left alone.** A folder or link Dalo does not own is
reported as unmanaged, never replaced. Local experiments in
`~/.claude/skills` keep working while the shared set is managed around them.

**Shared instructions too.** Instruction packs render into a marked block
inside your existing `CLAUDE.md` and leave everything you wrote around it
untouched. Every sync refreshes the block.

**Machines keep themselves current.** `dalo autosync install --schedule hourly`
registers a recurring check with launchd, systemd, or cron, and it is fail
closed: pending approvals, a dirty source, or a security finding stops the run
and tells you which.

Being honest about the limits, because this is a security-adjacent tool:

- It does not make third-party skills safe. The preflight is pattern based, the
  approval is a human decision, and a skill that passes both can still try to
  steer your agent. The security page lists what is out of scope, including
  prompt injection, a compromised upstream account, and whatever a skill causes
  at runtime.
- macOS on Apple Silicon and Linux only. Windows works through WSL; there is no native Windows
  build in the 1.x line.
- If you just want to try one public skill quickly, `npx skills add` is less
  work and reaches many more agents. Dalo is for the case where a set has to be
  the same for several people and reviewed before use.

1.0 means the CLI surface is now fixed for the whole 1.x line: commands, flags,
exit codes, `--json` shapes, and every file it persists. Upgrading from any 0.x
since 0.6.0 needs no migration step.

macOS and Linux, single binary, MIT or Apache-2.0, no telemetry, no account.

- Release notes: https://dalo.sh/news/1-0.html
- Docs: https://dalo.sh/docs/
- Repository: https://github.com/sebastian-software/dalo

Happy to answer questions here.

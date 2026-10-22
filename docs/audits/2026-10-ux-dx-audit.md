# Dalo UX/DX audit, October 2026

Goal of the brief: make Dalo maximally easy to use. Overseeable. Pleasant.
Positively simple. This document checks how close 1.10.0 is, and what the
shortest path to that feeling looks like.

Scope: CLI experience, developer experience, themeability, documentation, and
marketing. Written against commit `9536555` and the live site on 2026-10-23.

## The short version

Dalo is in good shape where it counts. Sync never touched a file it didn't own
(I tried). Every blocked state told me the exact next command. Bare `dalo` in
a terminal is the best "help" I've seen in a while: six lines of state and one
copyable command. The core promise holds.

It doesn't feel simple yet. It feels thorough. Those aren't the same thing,
and the gap is almost entirely surface: words, volume, and ordering. The
resolver, the lockfiles, the ownership rules, the ADRs: none of that needs to
move.

Verdict in product-decision terms: **conditionally ready** to call itself the
simple way to manage agent skills. The conditions are the week-1 and week-2
items in "The plan I'd run". All of them are human output and copy.

Seven moves, ranked by relief per hour:

1. Silence the assistant notice in explicit stores. (S)
2. Stop calling "not synced yet" a drift. (S)
3. One line of audit output on the happy path, not seven per skill. (S)
4. Group the 26 top-level commands in help and show six by default. (M)
5. One sentence for what Dalo is, used everywhere. Today there are six. (S)
6. One story for `init`. Today there are three. (S)
7. A five-minute start page of about 80 lines in front of the 475-line
   walkthrough. (M)

And the product fact that outranks all of them: I couldn't find a single
outside user to learn from. "The riskiest assumption" at the end says what to
do about that.

Effort tags throughout: S is under a day, M is a few days, L is a week or
more.

## How I looked, and what I didn't

Evidence base:

- The repository at 1.10.0: `README.md`, `docs/`, `site/`, `npm/`,
  `skills/dalo`, `src/cli.rs`, `src/term.rs`, `src/assistant.rs`.
- The live site at desktop and phone width, the docs index, the reference
  page, getting started, and the OG card.
- A debug build walked through a throwaway store: first run, local skill,
  catalog select, approve, sync, team source, shadowing, a real folder in the
  way, a dirty checkout, adoption, dry run, typos, and every `-h`.
- Adoption signals from GitHub, crates.io, and npm.

What's missing: people. No interviews, no observation, no support threads to
mine. GitHub Discussions is empty, and every human-authored open issue is the
maintainer's. So everything here is an expert first-use review plus numbers.
Where I guess at behavior, I say "hypothesis".

Adoption signals as of 2026-10-23:

| Signal | Value |
| --- | --- |
| GitHub stars / forks | 2 / 1 |
| Open issues by someone other than the maintainer or a bot | 0 of 22 |
| Discussions | 0 |
| crates.io downloads, all time / last 90 days | 667 / 393 |
| npm `getdalo` downloads, last 30 days / last 7 days | 1,543 / 124 |

Treat the npm number with care. Launchers get pulled by scanners and CI far
more often than by hands.

## The tension underneath everything

Dalo has two front doors and one house.

**Solo Sam** wants two skills in Claude Code before lunch. Sam's mental model
is "a package manager for skills". Sam's question: where's
`dalo install cool-skill`?

**Team-lead Tina** wants one reviewed set on twelve laptops and no surprises
when someone pushes to the skills repo. Tina's model is "lockfile plus
review". Tina's question: who approved this?

The house is built for Tina. The store, sources, pins, approvals, ownership
rules, the preflight: all of it is Tina's furniture, and it's good furniture.
The front door is painted for Sam: "Just ask." That's the right wedge. But
once Sam walks in, every room is labeled in Tina's language.

Three models to keep apart:

| Model | What it looks like |
| --- | --- |
| User model | "Skills are packages. I add one, update one, remove one." |
| Designed model | store, sources, resolution, targets, gated by approvals and ownership |
| System model | `lock.toml`, `source-lock.toml`, `approvals.toml`, content hashes, provenance, drift |

Today the system model leaks straight into human output. `status` says
"lock drift" and "materialization blocks". `sync` ends every run with
"security preflight: deterministic checks only". `select` prints a
64-character content hash. Sam reads all of that as "something is wrong".
Nothing is wrong. Dalo is thinking out loud.

Problem statement:

> When a developer tries Dalo for the first time, they need to get one skill
> into one agent and feel done, but the CLI and docs narrate internal state
> at every step, which leads them to believe they broke something, or to
> decide the tool is for "serious teams" and leave.

Empowerment statement:

> Developers can already read a lockfile and judge a diff. They should be
> able to run Dalo for weeks without meeting the words "materialization" or
> "provenance", and meet them only when a decision needs them, without losing
> any of the safety those words protect.

The design principle that follows is progressive disclosure in three layers:

- **Layer 0, it just works.** Five commands (`dalo`, `sync`, `status`,
  `approve skill`, `doctor`) or one sentence to your agent. Output: what
  changed, what needs you, the next command. Nothing else.
- **Layer 1, trust when it matters.** Approvals, audits, conflicts,
  shadowing. Shown when they block something, in plain words, with the
  command that clears them.
- **Layer 2, everything.** `--json`, `--all`, the reference. Every detail
  stays here. The JSON doesn't change.

The compatibility contract makes this cheap. Human-readable output is
explicitly outside the 1.x promise
([compatibility.md, Tier 3](../compatibility.md#tier-3-not-covered)), so
wording can change in a minor release. Nothing below asks to reopen an ADR.

## Scorecard by journey stage

| Stage | Today | Call |
| --- | --- | --- |
| Discover | Six different one-liners; the strongest hides in the GitHub About | Fix |
| Evaluate | Honest comparison, strong security page, 1,695-word landing | Trim |
| Start through the agent | One pasted sentence; depends on the host finding the skill | Good |
| Start through the CLI | Four commands to the first skill, but a nag line on every one in a sandbox | Fix |
| Reach value | `status` in 0.06 s, sync never overwrites | Great |
| Repeat | `next` is excellent; `status` is a wall | Trim |
| Recover | Every blocked state names the command; the troubleshooting table is gold | Great |
| Get help | Templates and SUPPORT.md are fine; nobody's there yet | Later |

## Findings

### 1. Positioning and marketing

**M1. Six ways to say what Dalo is.** (S)

| Where | Line |
| --- | --- |
| README, site hero, OG card | Manage your agent's setup. Just ask. |
| Site `<title>` | AI agent setup manager |
| CLI `about` | One source of truth for the skills your AI agents run. |
| `Cargo.toml`, npm `description` | AI agent skill manager: version, review, and sync skills through your agent. |
| npm README | the Git-backed skill manager for AI agents |
| GitHub About | Your team's agent skills, versioned like code. |
| Site footer | Git-backed skills, instructions, and hooks for AI agents. Built in Rust. |

The GitHub About is the best of the lot. It names the buyer (team), the thing
(agent skills), and the promise (versioned like code) in seven words. I'd make
it the category line everywhere and keep "Just ask." as the hook under it:

> **Your team's agent skills, versioned like code.**
> One reviewed set of skills, instructions, and hooks, synced into every
> agent on every machine. Ask your agent, or use the CLI.

**M2. The hero stacks two promises.** (S) "Manage your agent's setup. Just
ask." sits on top of "Keep one reviewed setup in Git across your team's
agents and machines." Conversation is the how. The reviewed set is the why.
Lead with the why; the comparison table further down the same page already
shows it's the real differentiator against skills.sh (exact pins, explicit
approval, never overwrites). "Just ask" stays as the first button.

**M3. The landing page does enterprise persuasion to an empty room.** (M)
1,695 words, four animated illustrations, a comparison table, and a proof
strip with "50 releases", "86.9% coverage", "5 signed targets", and "4 audit
findings resolved". That's engineering proof. It reassures me as a
contributor. A visitor wants customer proof, and there isn't any yet, so
don't fake it. Two cheaper moves: cut the page to roughly half, and swap the
proof strip for one honest line ("Early. We run it on our own machines every
day. Here's what it does and doesn't promise.") plus a short "Who it's for /
who it isn't for" block. "50 releases" in particular reads as churn to a
skeptic.

**M4. No outside signal yet.** This is the one that matters. See "The riskiest
assumption".

**M5. The comparison snapshot is seven minors stale.** (S) It says
"September 2026, Dalo 1.3". Dalo is 1.10. Re-stamp it the next time it's
touched, or stamp it from `Cargo.toml` like the version slot on the landing
page.

**M6. The README is 401 lines.** (S) The top third is strong. The ASCII
diagram and the eleven-bullet "Safe by design" list are the security page in
a README costume. Three bullets and a link would do. Same for "Installation",
which can be the install guide's job.

### 2. Documentation

**D1. The front door is a 475-line walkthrough with a six-row "pick your
situation" table.** (M) Good content, wrong altitude for page one. Put a
"Five minutes" page in front of it: install, run `dalo`, link one agent, add
one source, sync, done, with the agent sentence as the alternative. About 80
lines. Move "Three words status uses" into troubleshooting, where it already
has a twin.

**D2. Three stories for `init`.** (S)

| Where | Story |
| --- | --- |
| README terminal path, getting started step 1 | run `dalo init` first |
| `dalo -h` footer | "Optional: dalo init." |
| Assistant guide | normal use initializes a missing store automatically |

Pick one. Since 1.10 creates the store on first use, I'd go with: "`dalo`
just works. `init` is for sandboxes and projects." Then make the README and
the guide say that too.

**D3. The tone is careful to the point of apology.** (M, mostly deletion)
"It is not a safety guarantee" follows every clean audit. "Never", "remains
preserved", and "does not take ownership by surprise" show up on nearly every
page. Each sentence is true. Together they read like a compliance appendix,
and the brief asks for pleasant. One rule fixes most of it: say what happens,
once, in the sentence where it happens, and park the caveats on the security
page. Example from `source add` with two clean skills, 13 lines today:

```text
added source company -> /…/store/sources/company/checkout
found 2 skills
security audit: company:release-notes
  content hash: 2382fd0f72ea63cb086fe8ada4b8d2705f04bb2baeffe11f4be8612fe9dac519
  coverage: complete
  result: clean
  note: no findings means no known issue was detected; it is not a safety guarantee
security audit: company:review
  content hash: 54cfd54768e6cbd5e6105408a0736c9eb6b081624572d1518af13ea220d58255
  coverage: complete
  result: clean
  note: no findings means no known issue was detected; it is not a safety guarantee
next: dalo sync
```

Proposed, two lines:

```text
added source company (2 skills checked, no findings)
next: dalo sync
```

The full block stays one command away in `dalo audit` and in `--json`.

**D4. Thirty nouns before lunch.** (M) Store, source, team source, catalog,
local source, target, slot, selection, approval, trust, audit, preflight,
finding, lock, lock drift, resolution, materialization, shadowed, unlinked,
unmanaged, owned, adopt, keep, promote, instruction pack, managed block,
plugin, tool, hook, binary, delivery, agent package, project declaration,
team manifest, autosync, companion, bundle. A newcomer needs eight: skill,
source, catalog, target, approve, sync, adopt, conflict. Two moves: a
one-screen glossary of those eight at the top of the docs, and a rename pass
on human output only (U3 and U8 below). JSON codes stay as they are.

**D5. The reference is one 2,672-line page.** (M) Great for grep and for
agents. The site gives it an "On this page" rail, which helps. Splitting it
by command group would help humans more. Low priority next to D1 and D3.

**D6. The guide's own sandbox skills fail the spec check.** (S) All three
`SKILL.md` examples in getting-started.md have no frontmatter. Follow the
guide literally and `dalo audit local:review` prints
`spec.frontmatter-missing`; an agent that follows the Agent Skills spec won't
list the skill at all. Add the four-line frontmatter to each example.

**D7. Keep.** The "fast recovery paths" table in troubleshooting. The docs
index grouping (Start here, Look things up, Evaluate and maintain).
`llms.txt` and `install.md` for agents. The comparison page's honesty. The
one-paragraph summary on the security page.

### 3. CLI experience

**U1. 26 top-level commands, 72 leaf commands, one flat list.** (M) Everyday
use needs five. The rest are real and should stay, but they shouldn't all
stand in the doorway. Group the short help, promote six, keep the full list
under `--help`. Sketch:

```text
Everyday     status  sync  next  doctor  approve  adopt
Setup        init  target  source
Team         team  promote  project  install  migrate
Packages     plugin  tool  hook  binary  agent  plan  instructions
Automation   autosync  assistant  completions
```

Clap doesn't group subcommands natively, but a `help_template` with
`display_order` and a hand-written command block does it in an afternoon.
Splitting `src/cli.rs` (6,223 lines) along the same groups is the natural
companion change.

Flag counts tell the same story:

| Leaf command | Flags in `--help` |
| --- | --- |
| `sync` | 7 |
| `source add` | 9 |
| `project add` | 11 |
| `team catalog add` | 12 |
| `approve skill` | 14 |
| `audit` | 15 |

Most of those are the five global flags repeated on every leaf. They could
drop to the bottom of leaf help, or out of it.

**U2. The assistant notice nags on every command in an explicit store.** (S,
highest relief per hour) In the sandbox that the README and `dalo -h` both
recommend ("Try safely: use --store with a temporary directory"), nine of the
eleven commands I ran printed this, two of them twice:

```text
assistant: An assistant already exists outside this store's bundled installation; keep its installer ownership and inspect it before switching.
```

Only `--version` and `--help` stayed quiet. The trigger is the global store's
own companion links in `~/.claude/skills/dalo` and `~/.agents/skills/dalo`,
which look "external" from a sandbox
([`src/assistant.rs:179`](../../src/assistant.rs)). So the people most
likely to hit this are exactly the ones following the safe-trial path. Fix:
in an explicit `--store` or `DALO_STORE` context, say nothing; keep the state
in `assistant status --json` and as one doctor info finding. In the global
store, print it once and remember that you did.

**U3. "Lock drift" right after adding a skill.** (S) Create a local skill,
run `status`, and before the first sync you get:

```text
lock drift:
  active_added local:review: active skill `local:review` is not present in the lock
```

Sam reads "drift" as "broken". The guide knows this and pre-apologizes
("status may show a lock drift block until the next sync"). When every drift
item is an addition or removal the next sync will record, say that instead:

```text
waiting for sync:
  review (local)        run: dalo sync
```

Keep the "lock drift" heading for the genuinely surprising cases: the lock
says X, the live resolution says Y, and nobody asked for it.

**U4. The happy path talks too much.** (S) Every `sync` ends with
`security preflight: deterministic checks only`. Every `select`, `approve`,
`adopt`, and `source add` prints a five-to-seven-line audit block per skill
with a 64-character hash and the "not a safety guarantee" note. A 30-skill
team repo on `source add` is roughly 150 lines of clean results. Collapse to
one line per command on the happy path. Keep the full block for findings,
for `audit`, and for `--json`.

**U5. `status` is a wall.** (M) A sample line:
`local        local priority=0    skills=1   agents=0   plugins=0   enabled`,
followed by provenance lines with commit hashes. For a newcomer, more than
half of a healthy `status` is zeros and hashes. Proposed default shape:

```text
dalo store ~/.dalo
targets   claude  codex
skills    4 active   1 waiting for approval   1 blocked

needs you
  review-helper (public)     waiting for approval          run: dalo approve skill public:review-helper
  standup-notes (company)    a folder you own is in the way run: dalo adopt standup-notes

next: dalo approve skill public:review-helper
```

Everything else moves under `status --all` and stays in `--json`. `--check`
keeps its exit semantics.

**U6. `doctor` hides its info findings from humans.** (S) On a healthy
sandbox: `summary: errors=0 warnings=0 info=1 ok=16`, then
`details: 17 info/ok findings omitted; use --json for the full report`. The
one info item was "autosync not installed". Print info items; there are
never many. Or add `--verbose`.

**U7. Keep: `next`.** Bare `dalo` in a terminal prints a six-line summary and
one copyable next command. That's the heart of the CLI, and nearly every
command already ends with a `next:` line (`source add-catalog` does it
nicely: "2 skills available; next: dalo source inspect public, then dalo
source select public <skill>"). Make it a rule: no command ends without one.

**U8. One conflict, four sections.** (S) With a real folder in the way,
`status` reports the same fact under `skill delivery`,
`materialization blocks`, `resolution diagnostics`, and `unmanaged skills`.
`sync` then repeats both `local_override` and `shadowed` diagnostics on every
run, forever. Say it once under "needs you", and stop repeating diagnostics
that describe a steady state the user chose. A local override is a feature,
not a diagnostic.

The blocked line deserves a wording pass too. Today:

```text
blocked  conflict   target[generic]:/standup-notes -> store:/sources/company/checkout/skills/standup-notes (real unmanaged entry exists at target slot) (adopt: run `dalo adopt 'standup-notes'` to copy it into the local source; use `dalo adopt 'standup-notes' --replace` to replace the original)
```

Proposed:

```text
blocked  standup-notes    a folder you own is already there
         keep yours:      dalo resolve keep standup-notes
         bring it in:     dalo adopt standup-notes --replace
```

**U9. Keep: errors.** `synk` suggests `sync`. `cursr` suggests `cursor` and
lists all seven targets. `standdup` suggests `standup`. A missing source lists
the known sources. `adopt whatever` points at `resolve list`. This is the
"recovery, not dead ends" promise from the landing page, and it holds.

**U10. Keep: `--dry-run` in both positions.** `dalo --dry-run sync` and
`dalo sync --dry-run` both work, and the dry run says what it did not do
("did not refresh tracking source `company`").

**U11. Colors are three labels and a checkmark.** (S) Green for `ok`,
`applied`, and `existing`; yellow for `blocked` and `conflict`; red for
`error`. `NO_COLOR` and `TERM=dumb` are respected. There's no `--color` flag,
so CI logs and `| less -R` lose it. Minimal is fine. I'd add bold to the
`next:` line and a yellow heading for "needs you", nothing more, plus
`--color always|never|auto` because it's twenty lines and people will ask.

**U12. Paths are long and repeated.** (S) Every command opens with
`dalo store: /private/var/…/store`, and `status` repeats full target paths.
Home is already shortened to `~`, so this mostly bites in sandboxes. Low
priority.

### 4. Themeability

I read "Themebarkeit" two ways and checked both: visual theming of the site
and CLI, and the README theme pipeline.

**T1. The site is light only.** (M) `color-scheme: light` in both
stylesheets, no `prefers-color-scheme` block. The README already ships a dark
logo variant for GitHub, so the brand has thought about it. Dev-tool visitors
live in dark terminals; a dark palette is table stakes, not polish. The good
news: every color is a token on `:root`, so a dark set is a few dozen lines
plus a pass over the four illustrations. I'd do it after the copy work, not
before.

**T2. The CLI has no theme to speak of.** See U11. Fine as a baseline. A tiny
semantic palette (ok, attention, error, next) is the whole ask.

**T3. The README theme pipeline works and is well documented.**
`README.md.src` plus mdtheme plus a pinned company-theme commit, with a
pre-push guard. The cost: a one-word README fix needs mise, mdtheme, and Git
access to the theme repository, and a contributor who edits `README.md`
directly fails CI. The header comment warns them. A "fixing a typo" paragraph
in CONTRIBUTING that names the file and the command is enough.

**T4. One brand source for the video and the OG card.** `video/src/brand.tsx`
feeds both renders from the site palette. Nice. The wrinkle: the renders
embed Hanken Grotesk and Geist Mono while the site uses system font stacks,
so the OG card and the page it links to don't quite match. Either load the
two fonts on the site (they're already self-hosted in `site/assets/fonts`) or
let the renders use system fonts. Cosmetic.

### 5. Developer experience

**X1. The preflight is 14 commands.** (S) CI mirrors them and
`tests/workflows.sh` keeps the lists in step, which is good discipline.
There's no "run the fast set" task; `mise.toml` only knows the README. Add
`mise run check` (fmt, clippy, `cargo test --locked`) and
`mise run check:full` (the CI list), so the PR template can say "run
`mise run check:full`" instead of listing fourteen lines.

**X2. Six source files over 3,000 lines, `cli.rs` at 6,223.** (L, but
incremental) Not a correctness problem. It makes U1 harder than it should be
and turns every CLI change into a merge-conflict magnet. Split `cli.rs` along
the help groups from U1 when you do U1.

**X3. Three generated artifacts to keep honest.** README, `site/docs/*.html`,
and `og.png`, each with its own tool (mdtheme, Node plus pnpm, Remotion). All
documented, all checked in CI. The onboarding cost is real: a docs-only
contributor needs Rust, Node 24, pnpm, mise, and mdtheme to see green
locally. A `mise run docs` task that runs the README and site steps would
halve the explaining.

**X4. Keep.** `AGENTS.md` is short and useful. Tests are offline by rule.
Conventional Commits with release-please. The ADR index framed as
"constraints, not proposals" is exactly right, and it kept this audit honest.

## What's already great

Worth saying out loud, because the findings list is long and the engine
deserves it:

- Sync never touched a file it didn't own.
- Every blocked state printed the command that unblocks it.
- Bare `dalo` is a status plus one next step. Steal this for everything.
- Typo suggestions and known-ID lists everywhere.
- `status` in 0.06 seconds.
- A dry run that says what it skipped.
- The troubleshooting table, the comparison page, the security summary.
- `llms.txt` and `install.md`: an agent can install Dalo without a human
  reading anything.
- The site holds up at phone width, and the OG card is clean.
- Sigstore provenance on every release, and an installer that verifies by
  default.

## The plan I'd run

**Week 1, the quiet-down release.** U2, U3, U4, U6, D2, D6, M1, M5. All
small, all human output or copy, no schema changes. One PR each. Ship as a
minor: "Dalo talks less."

**Weeks 2 to 4, the front door.** U1 with the `cli.rs` split, U5, U8, D1, D3
as a tone pass over `docs/`, M2, M6, X1. Ship as "simpler by default."

**Later, once there are users to ask.** T1 dark theme, D5 reference split,
M3 landing trim, T4 fonts.

How to know it worked, without telemetry (there is none, and the privacy
stance says keep it that way):

| Measure | Today | Target |
| --- | --- | --- |
| Lines from a clean `sync`, 3 skills, 1 target, two local overrides | 11 | 4 |
| Lines from `source add` with 2 clean skills | 13 | 2 |
| Top-level commands shown by `dalo -h` | 26 | 6, rest under `--help` |
| Sentences that say what Dalo is, across README, site, crates, npm, GitHub | 6 | 1 |
| Stories for `init` across README, help, assistant guide | 3 | 1 |

## The riskiest assumption, and how to test it

Everything in this audit rests on two things nobody outside the company has
confirmed:

1. Developers want to manage agent skills by talking to their agent.
2. Teams want a reviewed, pinned skill set badly enough to adopt a new tool
   for it.

Both are plausible. Neither is observed. Two stars and an empty Discussions
tab aren't a verdict. They're an absence of data.

The smallest test that removes the most uncertainty: five outside
developers, two of them team leads, one hour each, screen shared. Task card:
"Get your team's skills repo (or this sample one) into the agent you use.
Then add one skill from a public catalog." No help. Note where they stop,
what they say out loud, and the time to the first synced skill. Run it before
the weeks-2-to-4 work, so that work fixes what real people hit rather than
what I hit.

Decision rule: if three of five reach a synced skill in under ten minutes
without opening the reference, keep the current model and do the polish. If
three of five stall on the same noun, that noun is the next PR, not the dark
theme. If nobody cares about the team part, that's a positioning
conversation, and a bigger one than this document.

## Appendix: transcripts behind the numbers

All runs used the debug build at 1.10.0 with `DALO_STORE` pointing at a
temporary directory, `DALO_OFFLINE=1`, and `DALO_UPDATE_CHECK=never`.

First run, the nag line (U2):

```text
$ dalo init
assistant: An assistant already exists outside this store's bundled installation; keep its installer ownership and inspect it before switching.
dalo store: /private/var/…/store
existing create_dir   store:/
created  create_dir   store:/local
…
Store ready.
Next steps:
  1. dalo target link <codex|claude|cursor|openclaw|hermes|opencode|generic> [path]
  …
```

Bare `dalo` in a terminal (U7, keep):

```text
Dalo
  store: /private/var/…/store
  initialized: yes
  linked targets: 1
  sources: 2
  active skills: 2
  pending approvals: 0

All synced ✓
  All configured skills are synchronized.
```

The conflict view in `status` (U8):

```text
skill delivery:
  company:standup-notes targets=generic mode=direct provider=- artifact=- blocked
    real unmanaged entry exists at target slot
…
materialization blocks:
  target[generic]:/standup-notes: real unmanaged entry exists at target slot
resolution diagnostics:
  local_override: local skill `local:review` overrides another source
  shadowed: skill `company:review` is unlinked because `local:review` wins the same slot
unmanaged skills:
  standup-notes -> target[generic]:/standup-notes (adopt: run `dalo adopt 'standup-notes'` to copy it into the local source; use `dalo adopt 'standup-notes' --replace` to replace the original)
```

A dirty team checkout (keep, this one is good):

```text
$ dalo sync
error: source `company` has local changes at `store:/sources/company/checkout`; inspect with `git -C 'store:/sources/company/checkout' status`, then resolve or commit them before syncing
[exit 3]
```

The `-h` footer, where the three `init` stories collide (D2):

```text
The packaged Dalo skill is set up on first use.
To manage additional skills: dalo target link <agent> -> add a team source, local skill, or adopted skill -> dalo sync
Optional: dalo init.
Use `dalo next` for one state-aware next action.
Try safely: use --store with a temporary directory and target link generic <path>.
```

## Outcome, 2026-10-23

The audit went straight into five pull requests on the same day:

| PR | What it does | Findings |
| --- | --- | --- |
| [#1022](https://github.com/sebastian-software/dalo/pull/1022) | Grouped `dalo -h` in six purpose headings, one tagline, one `init` story in the help footer, `--color`, bold `Next:` | U1, U11, D2, M1 |
| [#1023](https://github.com/sebastian-software/dalo/pull/1023) | One message everywhere (site, Cargo, npm, Nix, Debian, `llms.txt`), hero leads with the why, honest proof block and "who it's for", landing page from 1,695 to 1,247 words, comparison re-stamped | M1, M2, M3, M5 |
| [#1024](https://github.com/sebastian-software/dalo/pull/1024) | Assistant notice only on change and once per store, `waiting for sync` instead of lock drift, one-line clean audits, preflight trailer only when something was checked, `needs you` first in `status`, `status --all`, info findings in `doctor`, one conflict in one place with both ways out | U2, U3, U4, U5, U6, U8 |
| [#1025](https://github.com/sebastian-software/dalo/pull/1025) | Every color a token, dark palette following the OS, `data-theme` hook for a later toggle, logos swapped through `<picture>` | T1, T2 (site side) |
| [#1026](https://github.com/sebastian-software/dalo/pull/1026) | Five-minute start page with real transcripts, eight-word glossary, one `init` story in the docs, three status words moved to troubleshooting, frontmatter in every example, README trimmed, `mise run check`, `check:full`, `docs`, contributor notes | D1, D2, D3, D4, D6, M6, X1, X3, T3 |

Deviations from the plan above, on purpose:

- U1 shows all 26 commands grouped under six headings instead of hiding 20
  of them. Clap cannot hide commands from `-h` alone, and a grouped list
  turned out to read fine. The measure "6 shown, rest under `--help`" becomes
  "grouped, none flat".
- U8 went one step further than asked: a real folder that replaced a
  Dalo-owned link is now listed as unmanaged right away, so `status` and
  `sync` offer the same two ways out, and a folder without `SKILL.md` gets
  "move it aside" instead of commands that would fail.
- A pre-existing rendering bug came along for the ride: under a symlinked
  home directory (every macOS temp dir), sync printed the store path on both
  sides of the arrow. Fixed in #1024.

Left open, with a reason:

- M4, the five outside users. Not a pull request. Still the most important
  item on this page.
- D5, splitting the reference page. Low priority next to the rest.
- T4, site fonts versus the OG card. Cosmetic.
- `doctor` still prints its own `skill delivery` block for a conflict;
  #1024 scoped the dedupe to `status` and `sync`.
- X2, splitting `src/cli.rs`. Incremental, best done with the next CLI
  change that touches it.

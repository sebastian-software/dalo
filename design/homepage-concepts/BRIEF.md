# Homepage concept brief

Shared brief for the light homepage concepts in this folder. Read the
**Direction** section first: it is what the concepts are judged by. The facts
below it are the pool to draw from, not a list to cover.

## Direction (round 2)

Round 1 (newspaper, blueprint, man page, bento, Swiss poster, commit graph) was
rejected: the pages were far too full and several leaned on historical
metaphors. What is wanted instead:

- **A modern technology product.** Dalo should read like the tools that lead
  in product design today — Vercel, Stripe, GitHub, Linear, Supabase, Resend,
  Raycast. Crisp, confident, contemporary, precise. No retro, no historical or
  print metaphors (no newspapers, drawings, typewriters, man pages, posters).
- **Core messages first, details later.** Someone must understand what Dalo is
  and why it matters within five seconds of looking at the hero. Everything
  else is progressive disclosure: a few short sections, then links to the docs.
- **Restraint.** One message per section, one visual per section, generous
  whitespace. Headlines of a few words; supporting copy of one or two short
  sentences. Budget: about 250–400 words of visible copy on the whole page,
  excluding code, nav, and footer. At most 6–7 sections including hero and
  final CTA. No walls of cards, no long bullet lists, no tables of numbers.
- **The three core messages** (the page should be built around these):
  1. *One source of truth.* Your team's skills live in Git; Dalo resolves one
     pinned, approved set and links it into every agent's folder, the same on
     every machine.
  2. *Reviewed before it reaches an agent.* A local security preflight and an
     explicit approval gate every skill.
  3. *Safe by default.* Dalo only touches what it owns — never overwrites your
     files, shows every change with `--dry-run`.
  Supporting (short, lower on the page): works with Codex, Claude Code,
  OpenClaw, Hermes, OpenCode and any folder; three-command quickstart; open
  source, single Rust binary.
- **Light** theme, polished product visuals (UI-like cards, a clean light
  terminal or diagram, subtle depth/gradients where it fits the direction).
  Motion is welcome but must degrade to a finished static state under
  `prefers-reduced-motion` (screenshots are taken that way).

 Every concept is a
single self-contained `NN-slug/index.html` with inline CSS (and at most a small
inline script for progressive enhancement). Fonts come from `../fonts/*.css`
(self-hosted, latin subset). No CDN, no external images, no tracking — the same
rules the production site follows. All concepts are **light** designs.

## What Dalo is (the facts every concept must get right)

- Tagline: **"Your team's agent skills, versioned like code."**
- One-liner: Dalo keeps your team's agent skills in Git: one approved set,
  resolved the same way on every machine and reviewed before it reaches any
  agent folder.
- Audience: engineers and team leads who run Claude Code, Codex, or another
  agent across several people and machines.
- Rust CLI, single binary, no daemon, just `git` on PATH. macOS (Apple Silicon)
  and Linux (x86_64, ARM64). MIT OR Apache-2.0. By Sebastian Software.
- Core rule: **"Dalo may manage what it owns. It does not take ownership by surprise."**
- Model: sources (local, team Git repos, public catalogs) → `~/.dalo` store
  (sources · locks · trust · audits) → security preflight + approval →
  deterministic resolver → symlinks into agent folders. The store is the source
  of truth; agent folders are output targets.
- Resolver: pure function; same inputs → byte-for-byte same result. Source
  priority decides same-name conflicts, lower number wins (local prio 0 wins
  over company prio 10 over oss prio 20); losers are reported as `shadowed`.
- Safety: never overwrites unmanaged files or real folders; removes only links
  it owns; dirty team checkouts block refresh instead of losing edits; catalog
  skills need explicit selection + approval; `--dry-run` everywhere; conflicts
  stay visible in `status`.
- Security preflight: 15 deterministic rules run locally before a link is
  created; high/critical findings block until an explicit content-bound risk
  acceptance; optional sandboxed agent reviewer (Codex, Claude, OpenCode CLI)
  can add findings but never clears one. A passing preflight is not a safety
  guarantee — say so honestly if you mention it.
- Recovery: every blocking state names the next command; did-you-mean for
  typos; `dalo doctor` turns health into findings with a fix each.
- More: public catalogs (select only what you want, pinned), team manifests
  (`dalo.toml`, `dalo team catalog add`, `+copywriting` include, `-seo-audit`
  exclude, `skills = []` selects all), instruction packs (managed blocks inside
  agent instruction files, everything outside stays yours), autosync
  (launchd / systemd user timer, never grants approvals), portable plugins
  (`PLUGIN.toml`, tools and hooks approved separately by SHA-256 of their exact
  contract), adopt (bring an agent-written skill under management).

## Agents / targets

| Agent | Default skill directory |
| --- | --- |
| Codex | `~/.agents/skills` |
| Claude Code | `~/.claude/skills` |
| OpenClaw | `~/.agents/skills` |
| Hermes | `~/.hermes/skills` |
| OpenCode | `~/.config/opencode/skills` |
| Any folder-based agent | `dalo target link generic <path>` |

## Install

```sh
brew install sebastian-software/tap/dalo          # macOS
curl -fsSL https://dalo.sh/install.sh | sh        # macOS + Linux
npx getdalo --version                             # Node 20+
cargo binstall dalo  /  cargo install dalo  /  mise use -g github:sebastian-software/dalo
```

Agents can install it themselves via https://dalo.sh/install.md and /llms.txt.

## Quickstart (real commands)

```sh
dalo init
dalo target detect
dalo target link codex
dalo target link claude
dalo source add company git@github.com:acme/agent-skills.git
dalo sync
dalo status
```

Real sync output:

```text
applied  create     target[codex]:/incident-review -> store:/sources/company/checkout/skills/incident-review
applied  create     target[codex]:/release-notes -> store:/sources/company/checkout/skills/release-notes
synced: 2 skills across 1 target (2 created)
security preflight: deterministic checks only
```

Catalog + approval flow:

```sh
dalo source add-catalog sebastian https://github.com/sebastian-software/skills.sebastian-software.com.git
dalo source select sebastian effective-web
dalo sync
# synced: 2 skills across 1 target (2 unchanged)
# pending approval: sebastian:effective-web (run: dalo approve skill sebastian:effective-web)
dalo audit sebastian:effective-web
dalo approve skill sebastian:effective-web
dalo sync
```

Recovery output:

```text
$ dalo synk
error: unrecognized subcommand 'synk'
  tip: a similar subcommand exists: 'sync'

$ dalo approve skill company:relese-helper
error: skill `company:relese-helper` was not found; did you mean `company:release-helper`?; known skills: company:new-skill, company:release-helper
```

## Checkable numbers (use as-is)

- 34 releases since 0.1, all public
- 1011 tests behind an 86.9% line-coverage gate
- 5 signed release targets (SHA-256 checksum + Sigstore provenance each), 6 install channels
- 4 findings from the 2026-09 audit round, all closed
- 5 verified agents plus any folder
- Stores written by 0.6.0 or later open with no migration command, no re-approval

## Compared with

skills.sh is great for discovering public skills and installing them into
almost any agent; agentfiles puts agent files into an Obsidian UI. Dalo covers
the team side: exact commit pins, explicit approval, sync that never overwrites
unmanaged folders. "Find a skill on skills.sh, then let Dalo pin and deliver it."

## Links

GitHub https://github.com/sebastian-software/dalo · Docs /docs/ · Spec /spec/ ·
Security /docs/security.html · Comparison /docs/comparison.html · Install guide /install.md

## Brand

Name is written lowercase `dalo` as a wordmark, "Dalo" in prose. Existing mark:
a rounded square with three dots on the left feeding lines into one accent dot
on the right (many sources → one resolved set). Current accent is a rust
orange (#e8623a). Concepts may reinterpret color and type freely, but should
feel like the same honest, precise, slightly understated engineering product.

# Dalo

**One source of truth for the skills your AI agents run.**

[![Crates.io](https://img.shields.io/crates/v/dalo.svg)](https://crates.io/crates/dalo)
[![CI](https://github.com/sebastian-software/dalo/actions/workflows/ci.yml/badge.svg)](https://github.com/sebastian-software/dalo/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/rust-1.93%2B-orange.svg)](Cargo.toml)

Dalo turns scattered skill folders into shared, versioned infrastructure. Keep
team skills in Git, private experiments local, and deliver one approved,
deterministic skill set to Codex, Claude Code, OpenClaw, Hermes, or any
folder-based agent.

Your agents keep reading the folders they already understand. Dalo handles
everything behind them: sources, priorities, approvals, conflicts, drift, and
safe synchronization.

**[Visit dalo.sh](https://dalo.sh)** · **[Watch the 20-second demo](https://dalo.sh/#quickstart)** · **[Get started](docs/getting-started.md)**

## Stop copying skills between agents

Skills quickly become operational knowledge: how your team reviews code,
investigates incidents, ships releases, uses internal tooling, and makes the
judgment calls that generic prompts cannot capture.

Copying those skills by hand works until they matter. Then every machine has a
slightly different version, local improvements get lost, public skills change
upstream, and nobody can say with confidence what an agent is actually using.

Dalo gives that knowledge a lifecycle:

- **Share it in Git.** Team repositories stay reviewable and versioned.
- **Keep experiments local.** Private work remains separate until it is ready.
- **Resolve it predictably.** Source priority and lockfiles produce the same
  approved skill set from the same inputs.
- **Deliver it everywhere.** One sync links skills into every configured agent
  folder.
- **Stay in control.** Dalo reports conflicts and drift instead of overwriting
  files or silently trusting new code.

## See it work

Install Dalo on macOS or Linux:

```sh
# macOS with Homebrew
brew install sebastian-software/tap/dalo

# macOS or Linux with the hosted installer
curl -fsSL https://dalo.sh/install.sh | sh
```

Connect an agent, add your team's skill repository, and sync:

```sh
dalo init
dalo target detect
dalo target link codex
dalo source add company git@github.com:acme/agent-skills.git
dalo sync
```

The skills from `company` now appear in Codex's normal skill directory. Link
Claude Code, OpenClaw, Hermes, or a generic folder and Dalo will deliver the same
resolved set there too.

Run `dalo status` to see managed, unmanaged, shadowed, blocked, or pending
skills. Run `dalo doctor` when you want a focused health check.

Prefer Node.js? Run the same CLI without a global install:

```sh
npx getdalo --version
```

The quickstart is also available as a
[short MP4 video](https://dalo.sh/assets/dalo-quickstart.mp4).

## How Dalo fits

```text
 team repositories    public catalogs    local experiments
         \                  |                  /
          +-----------------+-----------------+
                            |
                            v
                     ~/.dalo store
             sources · locks · trust · audits
                            |
                            v
              preflight · agent review
                            |
                            v
                 deterministic resolution
                            |
             +--------------+--------------+
             v              v              v
           Codex       Claude Code     other agents
```

The store is the source of truth. Agent folders are output targets.

That boundary is what makes Dalo predictable: Git repositories remain clean
inputs, local work has a private home, and agent directories contain only the
resolved links Dalo owns. Existing unmanaged files remain yours.

## Built for the whole skill lifecycle

### Share team knowledge

A team source is a Git repository containing skills. Add it once and `sync`
refreshes clean tracking sources before resolving and linking the final set.

```sh
dalo source add company git@github.com:acme/agent-skills.git
dalo source priority company 10
dalo sync
```

When multiple sources offer the same skill name, source priority decides which
one is linked. The other candidates remain visible as shadowed; they are not
silently discarded.

Install safe recurring synchronization with the native user scheduler:

```sh
dalo autosync install --schedule daily
dalo autosync status
```

macOS uses launchd. Linux uses a systemd user timer when available and falls
back to an isolated, marked crontab entry. Scheduled runs never wait on an
interactive Dalo process, never grant approvals, and leave their latest
success, skip, or blocking reason visible in `status` and `doctor`.

#### Compose external skill sets for the team

A team repository can include a `dalo.toml` manifest alongside its own
`skills/` directory. Manage it from that repository with the team CLI:

```sh
dalo team init company --name "Company Skills"
dalo team catalog add marketing https://github.com/coreyhaines31/marketingskills.git \
  --version 0123456789abcdef0123456789abcdef01234567 \
  --skill +copywriting \
  --skill +launch
dalo team catalog skills marketing +copywriting +launch +seo-audit -seo-audit
dalo --dry-run team catalog update marketing --from main
dalo team catalog update marketing --from main
dalo team show
```

Use `--repo <path>` to manage another checkout. These commands only edit the
team repository; they do not require an initialized personal Dalo store and do
not commit or push changes. The resulting manifest pins external catalogs and
defines the subset that every team member should resolve:

```toml
schema_version = 1

[source]
id = "company"
name = "Company Skills"
kind = "team"

[[catalog]]
id = "marketing"
url = "https://github.com/coreyhaines31/marketingskills.git"
version = "0123456789abcdef0123456789abcdef01234567"
skills = ["+copywriting", "+launch", "+seo-audit", "-seo-audit"]
```

`version` accepts a Git commit, tag, or ref; an immutable commit is the most
reproducible choice. `team catalog update --from <ref>` clones into temporary
storage, previews selected-skill drift and deterministic audits, and writes the
resolved exact commit only when the candidate is safe. It never commits or
pushes the team repository. Skill filters follow these rules:

- omitted or empty `skills` selects everything
- only `-name` entries select everything except those entries
- any `+name` entry switches to whitelist mode
- exclusions always win, independent of entry order
- bare names are accepted as includes for compatibility

The catalog above appears locally as `company.marketing`. URL, version,
priority, and selection remain owned by the team manifest, while security
approval remains personal. After the first sync, each team member reviews the
pending skills and approves an appropriate scope before they are linked.

### Adopt what works locally

Agents often create useful skills directly in their own folders. Dalo can copy
one into the private local source without taking over the original:

```sh
dalo status
dalo adopt release-notes.local
```

Replacing the original with a Dalo-owned link is a separate, explicit step:

```sh
dalo adopt --replace release-notes.local
```

Dalo does not commit adopted work automatically. You decide when an experiment
is ready to move into a reviewed team repository.

### Choose from public catalogs

A catalog can offer many skills without installing all of them. Inspect it,
select only what you need, approve the exact skill, and sync:

```sh
dalo source add-catalog sebastian https://github.com/sebastian-software/skills.sebastian-software.com.git
dalo source inspect sebastian
dalo source select sebastian pr-review
dalo audit sebastian:pr-review --agent auto
dalo approve skill sebastian:pr-review
dalo sync
```

Catalog selections are pinned. `dalo source refresh <id>` reports when selected
skills change, move, disappear, or gain same-catalog dependencies without
silently advancing the lock. Review the exact candidate with
`dalo --dry-run source refresh <id> --advance`, then rerun without `--dry-run`
to update that catalog's checkout, locks, selection, and affected target links
as one rollback-safe transaction.

### Share instructions that are not skills

Instruction packs keep reusable team conventions in versioned Markdown and
render them as clearly marked managed blocks inside agent instruction files.

```sh
dalo instructions enable engineering-defaults ~/.agents/AGENTS.md
dalo instructions list
```

Dalo only owns the marked block. Everything else in the file remains untouched,
and overlapping topics are reported as advisory warnings.

## Safe by design

Skill management should be boring in the best possible way. Dalo is deliberately
conservative:

- unmanaged files and real directories are never overwritten during sync
- Dalo removes or repairs only the links and managed blocks it owns
- dirty team checkouts block refresh instead of losing local changes
- catalog skills require an explicit selection and approval
- source additions and catalog selections show deterministic security preflights
- `sync` blocks high and critical findings from deterministic checks and compatible cached reviews before a skill reaches an agent folder
- optional sandboxed reviews can reuse an installed Codex, Claude, or OpenCode CLI
- `sync` does not start an agent reviewer, and a passing preflight is not a safety guarantee
- newly active skills are not silently trusted during non-interactive sync
- `--dry-run` shows planned changes without writing them
- conflicts stay visible in `status` until you resolve or intentionally keep them

The core rule is simple:

> Dalo may manage what it owns. It does not take ownership by surprise.

## Agent support

| Agent | Default skill directory |
| --- | --- |
| Codex | `~/.agents/skills` |
| Claude Code | `~/.claude/skills` |
| OpenClaw | `~/.agents/skills` |
| Hermes | `~/.hermes/skills` |
| Any folder-based agent | user-provided path |

Cursor and OpenCode have experimental target IDs and currently require an
explicit path. See [agent integration](docs/agents.md) for setup details.

## Installation

Dalo supports macOS and Linux on x86_64 and ARM64. Windows is not currently
supported.

### Hosted installer

```sh
curl -fsSL https://dalo.sh/install.sh | sh
```

The installer selects the matching release, always verifies its SHA-256
checksum, and installs `dalo` into `~/.local/bin` by default. If
[`cosign`](https://docs.sigstore.dev/cosign/system_config/installation/) is on
`PATH`, it also verifies the release's Sigstore provenance.

Require provenance verification explicitly:

```sh
brew install cosign
curl -fsSL https://dalo.sh/install.sh | DALO_VERIFY=required sh
```

Set `DALO_INSTALL_DIR` to choose another binary directory. On unusual Linux
systems, `DALO_LINUX_LIBC=gnu` or `DALO_LINUX_LIBC=musl` overrides automatic
libc detection.

### Node.js

Use Node.js 20 or newer:

```sh
npx getdalo --version
# or
npm install --global getdalo
```

The small launcher downloads the matching release on first use, verifies its
SHA-256 checksum, and caches the executable in `~/.cache/dalo`.

### Package managers

```sh
# Homebrew (macOS)
brew install sebastian-software/tap/dalo

# Cargo Binstall
cargo binstall dalo

# mise with the GitHub Releases backend
mise use -g github:sebastian-software/dalo

# crates.io (requires Rust 1.93 or newer)
cargo install dalo
```

### Update notices

Successful interactive commands check for a newer GitHub release at most once
per day. The check is advisory: Dalo never replaces its own executable. When an
update is available, it prints the upgrade command for the detected installation
method once for that version. Network and cache failures are ignored, and checks
are disabled for JSON output, CI, and `DALO_OFFLINE=1`. Set
`DALO_UPDATE_CHECK=never` to opt out.

You can also ask your agent to install Dalo:

```text
Read https://dalo.sh/install.md and install dalo for me.
```

For manual archives, upgrades, shell completions, and removal, see the
[installation guide](https://dalo.sh/install.md) and
[uninstall guide](docs/uninstall.md).

## Documentation

- [Getting started](docs/getting-started.md)
- [Command reference](docs/reference.md)
- [Agent integration](docs/agents.md)
- [Dalo in CI](docs/ci.md)
- [Troubleshooting and FAQ](docs/troubleshooting.md)
- [Security policy](SECURITY.md)
- [Changelog](CHANGELOG.md)

Architecture decisions, RFCs, and implementation history remain available in
the [project documentation](docs/), but the CLI and command reference describe
the product you can use today.

## Development

Dalo is an MIT-licensed Rust CLI with a reusable library core.

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo run -- --help
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the complete development and release
workflow.

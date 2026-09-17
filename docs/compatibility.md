# Compatibility and Stability

This page answers one question: **if I upgrade Dalo within the same major
version, what can break?**

It applies to the 1.x line. Dalo is still on 0.x, so nothing here is a promise
about a released 1.0 yet — it is the contract the project commits to honor
once 1.0 ships, and the rule the maintainers already work by. Until then a 0.x
minor may still change any of the surfaces below, and the
[changelog](../CHANGELOG.md) is the record of what did change.

Everything is sorted into three tiers. If a command, flag, file, field, or
variable is not named in tier 1, it is not a 1.x promise.

## Tier 1: Stable in 1.x

These surfaces change only in a major release, and only after a deprecation
period (see [Change policy](#change-policy)).

### Commands and flags

Every command and flag documented in the [command reference](reference.md)
under "Command Reference" and "Global Flags", with the meaning documented
there. New commands, new subcommands, and new optional flags are additive and
may arrive in any minor release.

The global flags are `--store <PATH>`, `--json`, `--yes`, `--dry-run`,
`-h`/`--help`, and `-V`/`--version`. `--yes` is a retained no-op kept for
existing scripts; it stays accepted for the whole 1.x line.

### Exit codes

| Code | Name | Meaning |
| --- | --- | --- |
| `0` | success | Command completed. |
| `1` | expected failure | User-actionable input or state problem. |
| `2` | usage error | Invalid arguments or flags; plain text even with `--json`. |
| `3` | unsafe state | Dalo refused to mutate because the state needs human attention. |
| `4` | environment problem | Dependency, path, Git, filesystem, or external command problem. |

A command that exits `0` today does not start exiting non-zero for the same
input within 1.x, and no existing code is reassigned to a different meaning.
New codes above `4` would be a breaking change and are reserved for a major.

### `--json` output

The top-level shape of every report listed in
[JSON Output Shapes](reference.md#json-output-shapes) is stable, including the
documented field names, their types, and the `snake_case` enum spellings in the
"Common status values" table. Errors keep the documented
`{"error":{"code":"...","message":"..."}}` envelope on stderr.

Within 1.x, fields are added but never removed or retyped, and no existing enum
value is renamed. **Consumers must ignore unknown fields and tolerate unknown
enum values**; that is the price of additive evolution. New enum values may be
introduced in a minor release, so parse defensively rather than exhaustively.

Reports that carry their own `schema_version` follow the same rule as persisted
files below:

| Report | Field | Current |
| --- | --- | --- |
| `InstallationPlan` (`dalo plan`) | `schema_version` | `1` |
| `AuditReport` (`dalo audit`, embedded audits) | `schema_version` | `1` |
| `ApprovalListReport` (`dalo approve list`) | `schema_version` | `1` |
| `PackageValidationReport` (`dalo plugin validate`) | `schema_version` | `1` |
| `PluginReviewReport` (`dalo plugin review`) | `schema_version` | `2` |

### Persisted files

Dalo reads and writes these files. Each carries a version field; Dalo rejects a
version it does not support instead of guessing.

| File | Location | Version field | Current |
| --- | --- | --- | --- |
| [`config.toml`](reference.md#configtoml) | store root | `version` | `2` (version `1` is read and migrated forward) |
| [`state.toml`](reference.md#statetoml) | store root | `schema_version` | `1` |
| [`lock.toml`](reference.md#locktoml) | store root | `schema_version` | `6` (versions `1`–`5` are read and migrated forward) |
| [`approvals.toml`](reference.md#approvalstoml) | store root | `schema_version` | `1` |
| [`source-lock.toml`](reference.md#source-locktoml) | store root | `schema_version` | `3` (versions `1` and `2` are read and migrated forward) |
| [`dalo.toml`](reference.md#team-repository-dalotoml) | team repository root | `schema_version` | `1` |
| [`PLUGIN.toml`](reference.md#plugintoml-portable-plugins-tools-and-hooks) | `plugins/<name>/` in a source | `schema_version` | `1` (tool descriptor `1`, hook descriptor `1`) |

Two further author-facing files are versioned the same way and follow the same
rule: [`DELIVERY.toml`](reference.md#deliverytoml-provider-builds) provider
builds (`schema_version = 1`) and
[`AGENT.md`](reference.md#agentmd-canonical-packages) canonical agent packages
(`schema_version: 1` in frontmatter).

The promise is: a file written by a 1.x Dalo is readable by every later 1.x
Dalo, and a file you hand-authored against the documented schema keeps working.
The internal layout *around* those files is not covered — see
[Tier 3](#tier-3-not-covered).

### Environment variables

| Variable | Read by | Purpose |
| --- | --- | --- |
| `DALO_STORE` | CLI | Override the default store path; `--store` takes precedence. |
| `DALO_GIT_TIMEOUT_SECS` | CLI | Positive timeout in seconds for every Git subprocess. |
| `DALO_OFFLINE` | CLI | Disable passive update checks when truthy. |
| `DALO_UPDATE_CHECK` | CLI | `never` disables passive update checks. |
| `DALO_INSTALL_CHANNEL` | CLI | Launcher-provided installation context used for the upgrade hint. |
| `NO_COLOR` | CLI | Disable ANSI color output when set. |
| `DALO_TARGET` | `install.sh` | Release target override; see [installer variables](../site/install.md#installer-environment-variables). |
| `DALO_VERIFY` | `install.sh` | Signature-verification mode. |
| `DALO_LINUX_LIBC` | `install.sh`, npm launcher | Force `gnu` or `musl` selection. |
| `DALO_INSTALL_DIR` | `install.sh` | Destination directory for the binary. |
| `DALO_VERSION` | `install.sh`, npm launcher | Pin the version to fetch. |
| `DALO_CACHE_DIR` | npm launcher | Cache directory for downloaded archives. |

An unset variable always keeps the documented default behavior. Within 1.x,
none of these is removed and none changes meaning.

### Install channels

| Channel | Name |
| --- | --- |
| Hosted installer | `curl -fsSL https://dalo.sh/install.sh \| sh` |
| npm / npx | [`getdalo`](https://www.npmjs.com/package/getdalo) |
| Homebrew | `brew install sebastian-software/tap/dalo` |
| mise | `mise use -g github:sebastian-software/dalo` |
| Cargo | `cargo install dalo` |
| Release archives | GitHub Releases, tag `dalo-v<version>` |

Each channel keeps publishing every 1.x release. Retiring a channel is a
breaking change for the people who use it and happens only in a major, with the
usual one-minor deprecation notice.

### Agent targets

Target IDs are stable in the sense that an ID documented as supported keeps
working and keeps its meaning. Which targets are supported, and at which level,
is tracked in the agent matrix rather than promised here — see
[agent integration](agents.md) and `dalo target detect`, whose JSON reports each
target's `support` level (`supported` or `experimental`).

## Tier 2: Experimental

Experimental surfaces are usable and documented, but they may change or be
withdrawn in any release, including a patch. They are always labeled where they
appear.

| Surface | Where it is labeled |
| --- | --- |
| The Portable Agent Packages specification, draft 0.1 | [`docs/spec/README.md`](spec/README.md) — "experimental author-facing profile", a proposal for interoperability, not an adopted standard |
| Provider plugin and hook projections (the native files under `plugins/` and `hooks/` in the store, and the provider mappings behind them) | [`docs/spec/compatibility.md`](spec/compatibility.md) — evidence at pinned provider baselines, not a promise of equivalent semantics |
| Targets whose `support` is `experimental` (`cursor` and `opencode` today) | [`dalo target detect`](reference.md#dalo-target-detect) and its `TargetSupport` field |

Two distinctions are worth stating plainly:

- The `PLUGIN.toml` **file format** that Dalo reads is tier 1. The
  cross-implementation **specification** built on it is a draft, and so is any
  claim that another tool interprets it the same way.
- A provider projection is experimental because the *provider's* native format
  is outside Dalo's control. The portable package you authored stays stable;
  the compiled output for a given harness may need to change when that harness
  changes.

## Tier 3: Not covered

These may change in any release without notice.

- **Human-readable output.** Wording, layout, color, ordering, hints, and the
  suggested next command in text mode. Script against `--json` and the exit
  codes instead. Scraping text output is the single most common way to build a
  brittle integration.
- **Store-internal layout.** Everything under the store that is not a persisted
  file named in tier 1: `sources/`, `local/`, `tools/`, `generated/`,
  `audits/`, `hooks/`, `plugins/`, `catalog-advance.toml`, `autosync.toml`,
  `autosync-run.toml`, the log files, and the lock files `.lock` and
  `.catalog.lock`. Read them with `dalo` commands, not directly.
- **The Rust library API.** The Rust library API is not a semver contract; the
  CLI, its exit codes, its `--json` output, and the files Dalo persists are.
  See [The Rust library API](#the-rust-library-api).
- **Diagnostic and finding identifiers as an exhaustive set.** Individual codes
  documented in [troubleshooting](troubleshooting.md) keep their meaning, but
  new codes appear as Dalo learns to detect more, so treat the set as open.
- **Upstream behavior.** What a given agent harness does with a materialized
  skill, instruction block, or projected hook.

## Change policy

1. **Breaking changes only in a major release.** Removing or renaming a
   command, flag, exit code, JSON field, environment variable, or persisted
   field is a major-version change.
2. **Deprecations are announced one minor ahead.** Anything scheduled for
   removal in the next major is marked deprecated at least one minor release
   earlier, documented as deprecated in the reference, and emits a runtime
   warning on stderr when used. The warning never changes the exit status and
   is suppressed in `--json` mode so machine consumers stay clean.
   `--agent` (superseded by `--reviewer`) is a retained deprecated alias that
   predates this policy: it keeps working without a warning and is documented
   as deprecated in the reference. The warning requirement applies to
   deprecations announced from now on.
3. **JSON fields are never removed within 1.x.** A field that becomes
   meaningless keeps being emitted with a documented inert value until the next
   major. New fields are additive; consumers ignore what they do not know.
4. **Schema bumps migrate forward and fail closed on downgrade.** When a
   persisted schema version increases, a newer Dalo reads and rewrites the older
   file automatically. An older Dalo that meets a newer schema version refuses
   to operate with an actionable error rather than truncating, ignoring unknown
   fields, or rewriting the file at the version it understands. Downgrade is not
   supported; restore a backup or the previous store instead.
5. **Security fixes may break compatibility.** If the only safe fix for a
   vulnerability is a breaking change, it ships as soon as it is ready, is
   called out in the release notes, and the reasoning is recorded.

## Supported platforms

Dalo relies on Unix symlink APIs and `$HOME` resolution; a non-Unix build fails
at compile time by design.

| Platform | Architectures | libc |
| --- | --- | --- |
| macOS | `x86_64`, `aarch64` | system |
| Linux | `x86_64`, `aarch64` | `gnu` and `musl` |

The published release targets are `x86_64-apple-darwin`,
`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, and
`aarch64-unknown-linux-musl`.

**Windows is supported through WSL only.** Run Dalo inside a WSL Linux
distribution and point targets at paths inside that distribution. There is no
native Windows build, and native Windows is not planned for 1.x.

The minimum supported Rust version for building from source is the
`rust-version` field in `Cargo.toml`; every other mention of an MSRV is a
derived copy. Raising the MSRV is a minor-release change, not a major one, and
it never affects users who install a released binary.

## Support window

Security fixes land in the latest 1.x release. Older 1.x releases are not
patched separately, and there is no long-term-support branch: to receive a fix,
upgrade to the latest 1.x. Because the contract above forbids breaking changes
inside 1.x, that upgrade is meant to be uneventful.

When 2.0 ships, the 1.x line receives security fixes for six months from the
2.0 release date, so teams have a bounded window to migrate. See
[`SECURITY.md`](../SECURITY.md) for how to report a vulnerability.

## The Rust library API

`cargo add dalo` works and docs.rs renders the crate, but the library is not a
product. The Rust library API is not a semver contract; the CLI, its exit codes,
its `--json` output, and the files Dalo persists are.

Concretely:

- The crate exists so that CLI handlers stay thin and behavior can be tested
  without spawning the binary. Modules, types, function signatures, and trait
  implementations may change in any release, including a patch.
- Modules that are pure CLI plumbing are hidden from the rendered
  documentation. Being visible on docs.rs is not a stability signal either way.
- If you are integrating with Dalo, use the CLI: `--json` output plus the exit
  codes is the documented, supported integration surface, and it is covered by
  tier 1.
- If you need a stable Rust API, open an issue describing the use case. A
  curated library crate is a deliberate decision with its own cost, not
  something that happens by accident because a module was public.

This is recorded as
[ADR 0008](adr/0008-compatibility-contract.md).

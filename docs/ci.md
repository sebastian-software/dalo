# Using Dalo in CI

Passive release checks are disabled automatically when the conventional `CI`
environment variable is set. `DALO_OFFLINE=1` or `DALO_UPDATE_CHECK=never` can
also disable them explicitly in other managed environments.

Dalo's JSON output and exit codes are intended for automation.

Useful checks:

- `dalo status --check --json` reports resolution and fails when the state needs review, including malformed or unsafe portable agent packages.
- `dalo doctor --check --json` reports health and fails on error findings.
- `dalo sync --check --json` renders the sync result and fails when materialization is blocked or incomplete.
- `dalo source refresh <catalog> --check` checks catalog drift read-only and fails for changed, moved, or removed selected skills.

## Example GitHub Actions job

```yaml
name: dalo

on:
  pull_request:
  push:
    branches:
      - main

jobs:
  dalo:
    runs-on: ubuntu-latest
    env:
      DALO_STORE: ${{ runner.temp }}/dalo-store
    steps:
      - uses: actions/checkout@v7

      - name: Install dalo
        run: cargo install dalo

      - name: Configure a temporary Dalo store
        run: |
          dalo init
          dalo target link generic "$RUNNER_TEMP/dalo-skills"
          dalo source add project .
          dalo sync

      - name: Check dalo status
        run: dalo status --check --json > dalo-status.json

      - name: Check dalo health
        run: dalo doctor --check --json > dalo-doctor.json
```

The checkout in this example is a local Git source. Replace `.` with the path
or URL of the skill repository that the workflow should validate. The temporary
store and generic target keep the check isolated from any runner state.

## Release publication

The publish workflow keeps a new GitHub release as a draft while its six target
archives, checksums, and Sigstore bundles build and upload. The workflow creates
the release tag at draft time so each matrix job can check out the exact release
commit. A single final job verifies every expected asset and publishes the
release only after the complete matrix succeeds. If a build is failed or
cancelled, the incomplete release remains a non-public draft and the preceding
public latest release stays available.

GitHub is published before crates.io, npm, or the Homebrew tap dispatch. Each of
those downstream channels depends on the final GitHub-release job, so no public
installer path advertises an archive before GitHub makes that archive available.

### Releasing 1.0.0

`release-please-config.json` sets `bump-minor-pre-major: true`, so every
`feat!:` commit below 1.0.0 produces a minor bump, not a major. The first major
is requested explicitly, with a `Release-As:` footer on a commit that reaches
`main`:

```text
chore(release): release dalo 1.0.0

Release-As: 1.0.0
```

The footer is matched case-insensitively on the token `Release-As`, it must sit
in the commit footer, and it works with any Conventional Commit type — including
types the changelog normally hides, because the changelog generator keeps a
commit that carries the footer. Pull requests are rebase-merged, which preserves
the footer; a squash merge only preserves it when the squash body keeps it.
Other trailers such as `Co-Authored-By:` may follow it.

Release-please then opens `chore(main): release dalo 1.0.0` on
`release-please--branches--main--components--dalo`, writes the `## [1.0.0]`
CHANGELOG heading, and stamps `1.0.0` into `Cargo.toml`, `Cargo.lock`,
`npm/package.json`, both `npm/package-lock.json` version paths, and the three
annotated slots in `site/index.html` (`softwareVersion` plus the two
`data-dalo-version` spans). Replace the generated release body with the curated
launch notes from `.github/release-notes/<version>.md` before the draft is
published, as described under [Curated release notes](#curated-release-notes).

Merging that release pull request tags `dalo-v1.0.0` and runs `publish.yml`:
six signed archives with checksums upload to a draft release, the draft is
published once every asset is present, and crates.io, npm, and the
`sebastian-software/homebrew-tap` dispatch follow. The tap bump compares
versions with `sort -V`, so `1.0.0` supersedes `0.16.0` rather than losing to
it in string order.

Smoke every channel once the release page is public:

```sh
brew upgrade dalo && dalo --version
npx getdalo@latest --version
cargo binstall dalo && dalo --version
mise up && dalo --version
curl -fsSL https://dalo.sh/install.sh | sh && dalo --version
```

Then check the upgrade path itself from a 0.16 install: run any command under a
TTY with `DALO_UPDATE_CHECK` unset and `CI` unset, and confirm the notice reads
`update available: dalo v1.0.0 (installed v0.16.0 via <channel>)` with one `v`
per version and the upgrade command that matches the channel it was installed
from.

### Curated release notes

Release-please generates a commit list. For a release that a newcomer will read,
that list is the appendix, not the story, so the narrative is written ahead of
time and committed to the repository:

- The body for 1.0.0 is
  [`.github/release-notes/1.0.0.md`](https://github.com/sebastian-software/dalo/blob/main/.github/release-notes/1.0.0.md).
  A later release that needs the same treatment adds
  `.github/release-notes/<version>.md` beside it; releases without such a file
  keep the generated body unchanged.
- **Before the draft is published**, replace the draft release body with that
  file's contents, then a `---` divider, then the generated commit list
  unchanged. `publish.yml` only flips `--draft=false`; it never rewrites the
  body, so the edit survives publication.
- **After the release pull request is merged**, add the same text above the
  generated entry in `CHANGELOG.md` in a separate `docs:` commit. Release-please
  owns that file and appends new entries at the top, so the narrative goes in
  after its pull request rather than as a hand-written heading it has to parse.
- The notes repeat the breaking-change inventory that `docs/upgrading.md`
  carries, and `tests/docs.sh` checks both against the same list. A `!:` commit
  that lands before the tag has to be added to all three in one pull request.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Expected actionable failure, including semantic value validation, failed checks, and security-audit blocks |
| 2 | Usage error from invalid arguments or flags; emitted as plain text even with `--json` |
| 3 | Unsafe state blocked the operation |
| 4 | Dependency or environment problem |

Treat `1` as a user-actionable configuration or drift problem, `3` as a safety stop that should not be auto-fixed, and `4` as a runner/tooling problem.

## Catalog drift

For catalog sources, use the read-only refresh check. `--check` changes only
the exit status; neither form advances the catalog pin:

```sh
dalo source refresh company-catalog --check
```

The command reports new available skills, selected skill changes, moved or removed selections, and changed requirements without changing the source lock. New unselected skills remain informational; any selected-skill drift exits with code 1 for review.

Pin advancement is deliberately separate from CI checking. Preview the exact
transaction without writes, then apply it only in a reviewed maintenance flow:

```sh
dalo --dry-run --json source refresh company-catalog --advance
dalo source refresh company-catalog --advance
```

The advance report contains both lock entries, every drift classification,
the affected materialization plan, and blocking reasons. Never add `--advance`
to an unattended drift-check job.

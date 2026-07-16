# Using Dalo in CI

Passive release checks are disabled automatically when the conventional `CI`
environment variable is set. `DALO_OFFLINE=1` or `DALO_UPDATE_CHECK=never` can
also disable them explicitly in other managed environments.

Dalo's JSON output and exit codes are intended for automation.

Useful checks:

- `dalo status --check --json` reports resolution and fails when the state needs review.
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

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Expected actionable failure, including failed checks and security-audit blocks |
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

# Using Dalo in CI

Dalo's JSON output and exit codes are intended for automation.

Useful checks:

- `dalo status --check --json` reports resolution and fails when the state needs review.
- `dalo doctor --check --json` reports health and fails on error findings.
- `dalo source refresh <catalog> --check` checks catalog drift without advancing the pin and fails for changed, moved, or removed selected skills.

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
    steps:
      - uses: actions/checkout@v7

      - name: Install dalo
        run: cargo install dalo

      - name: Check dalo status
        run: dalo status --check --json > dalo-status.json

      - name: Check dalo health
        run: dalo doctor --check --json > dalo-doctor.json
```

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Expected actionable failure |
| 3 | Unsafe state blocked the operation |
| 4 | Dependency or environment problem |

Treat `1` as a user-actionable configuration or drift problem, `3` as a safety stop that should not be auto-fixed, and `4` as a runner/tooling problem.

## Catalog drift

For catalog sources, use the read-only refresh check:

```sh
dalo source refresh company-catalog --check
```

The command reports new available skills, selected skill changes, moved or removed selections, and changed requirements without changing the source lock. New unselected skills remain informational; any selected-skill drift exits with code 1 for review.

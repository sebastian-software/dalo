# Using Dalo in CI

Dalo's JSON output and exit codes are intended for automation.

Useful checks:

- `dalo status --json` reports resolution, lock drift, unmanaged target skills, and instruction pack drift.
- `dalo doctor --json` reports store, target, Git, lockfile, and instruction health.
- `dalo source refresh <catalog>` checks a catalog source for upstream drift without advancing the pin.

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
        run: dalo status --json > dalo-status.json

      - name: Check dalo health
        run: dalo doctor --json > dalo-doctor.json
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
dalo source refresh company-catalog
```

The command reports new available skills, selected skill changes, moved or removed selections, and changed requirements without changing the source lock.

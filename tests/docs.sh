#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

for document in "$root/README.md" "$root/site/index.html" "$root/site/install.md" "$root/docs/uninstall.md"; do
  grep -q 'npx getdalo' "$document"
done
grep -q 'npm uninstall --global getdalo' "$root/docs/uninstall.md"
grep -q 'dalo approve skill' "$root/docs/troubleshooting.md"
grep -q 'dalo approve skill' "$root/docs/getting-started.md"
grep -q 'dalo approve skill' "$root/site/index.html"
grep -q 'v0.6.0 release notes' "$root/README.md"
if grep -q 'version=0\.4\.1' "$root/README.md"; then
  echo "README manual installation version is stale" >&2
  exit 1
fi

echo "documentation checks passed"

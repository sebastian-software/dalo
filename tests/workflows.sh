#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/dalo-workflow-test.XXXXXX")"

cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT INT TERM

version_check="$({
  awk '
    /^[[:space:]]+test .*node -p.*package.json/ {
      sub(/^[[:space:]]+/, "")
      print
      found = 1
    }
    END { if (!found) exit 1 }
  ' "$root/.github/workflows/publish.yml"
})"

version_check_script="$test_root/publish-version-check.sh"
{
  printf '%s\n' 'set -eu'
  printf '%s\n' 'version="$1"'
  printf '%s\n' "$version_check"
} > "$version_check_script"

(
  cd "$root/npm"
  version="$(node -p 'require("./package.json").version')"
  bash "$version_check_script" "$version"
)

package_files="$(cd "$root" && cargo package --list --allow-dirty)"
for excluded_prefix in '.github/' 'docs/' 'npm/' 'site/' 'video/'; do
  if printf '%s\n' "$package_files" | grep -q "^$excluded_prefix"; then
    echo "cargo package unexpectedly contains $excluded_prefix" >&2
    exit 1
  fi
done

echo "workflow checks passed"

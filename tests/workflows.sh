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

workflow="$root/.github/workflows/publish.yml"
release_config="$root/release-please-config.json"

job_body() {
  awk -v job="$1" '
    $0 == "  " job ":" { found = 1; in_job = 1; next }
    in_job && /^  [A-Za-z0-9_-]+:/ { exit }
    in_job { print }
    END { if (!found) exit 1 }
  ' "$workflow"
}

test "$(node -p 'require(process.argv[1]).packages["."].draft' "$release_config")" = true
test "$(node -p 'require(process.argv[1]).packages["."]["force-tag-creation"]' "$release_config")" = true

artifacts_job="$(job_body release-artifacts)"
final_release_job="$(job_body publish-github-release)"
crate_job="$(job_body publish-crate)"
npm_job="$(job_body publish-npm)"
homebrew_job="$(job_body update-homebrew)"

printf '%s\n' "$artifacts_job" | grep -Fqx '    needs: release-please'
printf '%s\n' "$artifacts_job" | grep -Fq "gh release view \"\$TAG_NAME\" --json isDraft --jq '.isDraft'"
printf '%s\n' "$final_release_job" | grep -Fqx '    needs: [release-please, release-artifacts]'
printf '%s\n' "$final_release_job" | grep -Fq "needs.release-artifacts.result == 'success'"
printf '%s\n' "$final_release_job" | grep -Fq 'gh release edit "$TAG_NAME" --draft=false'
for target in \
  x86_64-unknown-linux-gnu \
  aarch64-unknown-linux-gnu \
  x86_64-unknown-linux-musl \
  aarch64-unknown-linux-musl \
  x86_64-apple-darwin \
  aarch64-apple-darwin; do
  printf '%s\n' "$final_release_job" | grep -Fq "$target"
done

for release_job in "$artifacts_job" "$final_release_job" "$crate_job" "$npm_job" "$homebrew_job"; do
  printf '%s\n' "$release_job" | grep -Fq "needs.release-please.outputs.release_created == 'true'"
done

for downstream_job in "$crate_job" "$npm_job" "$homebrew_job"; do
  printf '%s\n' "$downstream_job" | grep -Fq 'publish-github-release'
done

package_files="$(cd "$root" && cargo package --list --allow-dirty)"
for excluded_prefix in '.github/' 'docs/' 'npm/' 'site/' 'video/'; do
  if printf '%s\n' "$package_files" | grep -q "^$excluded_prefix"; then
    echo "cargo package unexpectedly contains $excluded_prefix" >&2
    exit 1
  fi
done

echo "workflow checks passed"

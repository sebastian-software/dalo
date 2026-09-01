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
ci_workflow="$root/.github/workflows/ci.yml"
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
test "$(node -p 'require(process.argv[1]).packages["."]["extra-files"].includes("npm/package.json")' "$release_config")" = true
test "$(node -p 'require(process.argv[1]).packages["."]["extra-files"].filter(entry => entry.path === "npm/package-lock.json").length' "$release_config")" = 2
(
  cd "$root/npm"
  npm run check-version
)
drifted_package="$test_root/package.json"
sed 's/"version": "[^"]*"/"version": "0.0.0"/' "$root/npm/package.json" > "$drifted_package"
if DALO_PACKAGE_JSON="$drifted_package" node "$root/npm/scripts/check-version.js" >/dev/null 2>&1; then
  echo "npm version check accepted a drifted package manifest" >&2
  exit 1
fi

artifacts_job="$(job_body release-artifacts)"
final_release_job="$(job_body publish-github-release)"
crate_job="$(job_body publish-crate)"
npm_job="$(job_body publish-npm)"
homebrew_job="$(job_body update-homebrew)"
release_please_job="$(job_body release-please)"

ci_job_body() {
  awk -v job="$1" '
    $0 == "  " job ":" { found = 1; in_job = 1; next }
    in_job && /^  [A-Za-z0-9_-]+:/ { exit }
    in_job { print }
    END { if (!found) exit 1 }
  ' "$ci_workflow"
}

ci_test_job="$(ci_job_body test)"
release_targets_job="$(ci_job_body release-targets)"
coverage_job="$(ci_job_body coverage)"

printf '%s\n' "$ci_test_job" | grep -Fq 'cargo test --locked'
printf '%s\n' "$ci_test_job" | grep -Fq 'cargo clippy --locked --all-targets --all-features -- -D warnings'
printf '%s\n' "$ci_test_job" | grep -Fq 'cargo build --release --locked --target "${{ matrix.target }}"'

# The host test job covers the two native release targets. The dedicated job
# covers the four remaining targets, including native ARM execution and the
# static-musl release-binary smoke path.
for target in x86_64-unknown-linux-gnu aarch64-apple-darwin; do
  printf '%s\n' "$ci_test_job" | grep -Fq "$target"
done

for target in \
  aarch64-unknown-linux-gnu \
  x86_64-unknown-linux-musl \
  aarch64-unknown-linux-musl \
  x86_64-apple-darwin; do
  printf '%s\n' "$release_targets_job" | grep -Fq "$target"
done

release_target_entry() {
  printf '%s\n' "$release_targets_job" | awk -v target="$1" '
    /^          - os:/ {
      if (entry ~ ("target: " target)) {
        found = 1
        print entry
        exit
      }
      entry = $0 ORS
      next
    }
    { entry = entry $0 ORS }
    END {
      if (!found) {
        if (entry ~ ("target: " target)) {
          print entry
        } else {
          exit 1
        }
      }
    }
  '
}

aarch64_gnu_entry="$(release_target_entry aarch64-unknown-linux-gnu)"
x86_64_musl_entry="$(release_target_entry x86_64-unknown-linux-musl)"
aarch64_musl_entry="$(release_target_entry aarch64-unknown-linux-musl)"
x86_64_darwin_entry="$(release_target_entry x86_64-apple-darwin)"

printf '%s\n' "$aarch64_gnu_entry" | grep -Fqx '          - os: ubuntu-24.04-arm'
printf '%s\n' "$aarch64_gnu_entry" | grep -Fqx '            builder: cargo'
printf '%s\n' "$aarch64_gnu_entry" | grep -Fqx '            test: cargo'
printf '%s\n' "$aarch64_gnu_entry" | grep -Fqx '            smoke: false'
printf '%s\n' "$x86_64_musl_entry" | grep -Fqx '          - os: ubuntu-latest'
printf '%s\n' "$x86_64_musl_entry" | grep -Fqx '            builder: cross'
printf '%s\n' "$x86_64_musl_entry" | grep -Fqx '            test: cross'
printf '%s\n' "$x86_64_musl_entry" | grep -Fqx '            smoke: true'
printf '%s\n' "$aarch64_musl_entry" | grep -Fqx '          - os: ubuntu-latest'
printf '%s\n' "$aarch64_musl_entry" | grep -Fqx '            builder: cross'
printf '%s\n' "$aarch64_musl_entry" | grep -Fqx '            test: none'
printf '%s\n' "$aarch64_musl_entry" | grep -Fqx '            smoke: false'
printf '%s\n' "$x86_64_darwin_entry" | grep -Fqx '          - os: macos-14'
printf '%s\n' "$x86_64_darwin_entry" | grep -Fqx '            builder: cargo'
printf '%s\n' "$x86_64_darwin_entry" | grep -Fqx '            test: none'
printf '%s\n' "$x86_64_darwin_entry" | grep -Fqx '            smoke: false'

printf '%s\n' "$release_targets_job" | grep -Fq 'runs-on: ${{ matrix.os }}'
printf '%s\n' "$release_targets_job" | grep -Fq 'cross test --locked --target "${{ matrix.target }}" --lib'
printf '%s\n' "$release_targets_job" | grep -Fq 'cargo test --locked --target "${{ matrix.target }}"'
printf '%s\n' "$release_targets_job" | grep -Fq 'target/${{ matrix.target }}/release/dalo'
printf '%s\n' "$release_targets_job" | grep -Fq '"$binary" init --store "$test_root/store"'
printf '%s\n' "$release_targets_job" | grep -Fq '"$binary" sync --store "$test_root/store" --dry-run'
printf '%s\n' "$coverage_job" | grep -Fq 'cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines 86.9'

printf '%s\n' "$artifacts_job" | grep -Fqx '    needs: release-please'
printf '%s\n' "$artifacts_job" | grep -Fq "gh release view \"\$TAG_NAME\" --json isDraft --jq '.isDraft'"
printf '%s\n' "$artifacts_job" | grep -Fq 'GH_REPO: ${{ github.repository }}'
printf '%s\n' "$release_please_job" | grep -Fq "inputs.recover_tag != ''"
printf '%s\n' "$release_please_job" | grep -Fq "gh release view \"\$TAG_NAME\" --json isDraft --jq '.isDraft'"
printf '%s\n' "$release_please_job" | grep -Fq 'GH_REPO: ${{ github.repository }}'
printf '%s\n' "$release_please_job" | grep -Fq 'echo "release_created=true" >> "$GITHUB_OUTPUT"'
printf '%s\n' "$release_please_job" | grep -Fq 'echo "tag_name=$TAG_NAME" >> "$GITHUB_OUTPUT"'
printf '%s\n' "$release_please_job" | grep -Fq 'echo "release_is_draft=$release_is_draft" >> "$GITHUB_OUTPUT"'
printf '%s\n' "$final_release_job" | grep -Fqx '    needs: [release-please, release-artifacts]'
printf '%s\n' "$final_release_job" | grep -Fq "needs.release-artifacts.result == 'success'"
printf '%s\n' "$final_release_job" | grep -Fq 'GH_REPO: ${{ github.repository }}'
printf '%s\n' "$final_release_job" | grep -Fq 'gh release edit "$TAG_NAME" --draft=false'
printf '%s\n' "$crate_job" | grep -Fq 'https://crates.io/api/v1/crates/dalo/${version}'
printf '%s\n' "$crate_job" | grep -Fq 'is already published on crates.io'
printf '%s\n' "$npm_job" | grep -Fq 'npm view "getdalo@${version}" version'
printf '%s\n' "$npm_job" | grep -Fq 'is already published on npm'
if printf '%s\n' "$npm_job" | grep -Fq 'npm version "$version"'; then
  echo 'npm publish must validate the release manifest without rewriting its version' >&2
  exit 1
fi
printf '%s\n' "$homebrew_job" | grep -Fq 'sort -V | tail -n 1'
printf '%s\n' "$homebrew_job" | grep -Fq 'not dispatching ${version}'

for downstream_job in "$crate_job" "$npm_job" "$homebrew_job"; do
  printf '%s\n' "$downstream_job" | grep -Fq "needs.release-please.outputs.release_is_draft == 'false'"
done
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

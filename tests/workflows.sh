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
# release-please keeps the version shown on dalo.sh in step with the crate.
test "$(node -p 'require(process.argv[1]).packages["."]["extra-files"].some(entry => entry.type === "generic" && entry.path === "site/index.html")' "$release_config")" = true
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
msrv_job="$(ci_job_body msrv)"
audit_job="$(ci_job_body audit)"

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
sh "$root/scripts/check-workflow-pins.sh" "$root/.github/workflows" > /dev/null

# The macOS test entry and the comment above it kept contradicting each other.
printf '%s\n' "$ci_test_job" | grep -Fq 'Keep macos-latest as the macOS test runner'
printf '%s\n' "$ci_test_job" | grep -Fqx '          - os: macos-latest'
if printf '%s\n' "$ci_test_job" | grep -Fq -e '- os: macos-14'; then
  echo 'the macOS test entry must stay on macos-latest; macos-14 rejects the hook fixtures' >&2
  exit 1
fi

# One coverage threshold, read from `coverage-threshold` by everything that
# states it.
coverage_threshold="$(cat "$root/coverage-threshold")"
case "$coverage_threshold" in
  ''|*[!0-9.]*)
    echo "coverage-threshold must hold a plain number, found '$coverage_threshold'" >&2
    exit 1
    ;;
esac
coverage_command='cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines "$(cat coverage-threshold)"'
for coverage_document in "$ci_workflow" "$root/CONTRIBUTING.md" "$root/.github/pull_request_template.md"; do
  grep -Fq "$coverage_command" "$coverage_document" || {
    echo "$coverage_document does not read the gate from coverage-threshold" >&2
    exit 1
  }
done
printf '%s\n' "$coverage_job" | grep -Fq "$coverage_command" || {
  echo 'the CI coverage job no longer reads the gate from coverage-threshold' >&2
  exit 1
}
if grep -Fq "fail-under-lines $coverage_threshold" "$ci_workflow" "$root/CONTRIBUTING.md"; then
  echo 'the coverage threshold must be read from coverage-threshold, not restated' >&2
  exit 1
fi

# The MSRV job takes its toolchain from `rust-version` so the number lives in
# Cargo.toml alone.
msrv="$(sed -n 's/^rust-version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)"
test -n "$msrv"
printf '%s\n' "$msrv_job" | grep -Fq 'steps.msrv.outputs.version'
if printf '%s\n' "$msrv_job" | grep -Fq "$msrv"; then
  echo 'the MSRV job must read rust-version from Cargo.toml, not restate it' >&2
  exit 1
fi

# The pull request checklist, the CONTRIBUTING preflight, and the CI jobs are
# one contract. Every command below has to appear in all three.
pr_template="$root/.github/pull_request_template.md"
contributing="$root/CONTRIBUTING.md"

for shared_command in \
  'cargo fmt --check' \
  'cargo test --locked' \
  'sh tests/install.sh' \
  'sh tests/docs.sh' \
  'sh tests/workflows.sh' \
  'npm ci && npm run check-version && npm test' \
  'cargo clippy --locked --all-targets --all-features -- -D warnings' \
  'cargo build --release --locked' \
  'RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features'; do
  printf '%s\n' "$ci_test_job" | grep -Fq "$shared_command" || {
    echo "the CI test job no longer runs: $shared_command" >&2
    exit 1
  }
  grep -Fq "$shared_command" "$pr_template" || {
    echo "the pull request checklist is missing: $shared_command" >&2
    exit 1
  }
  grep -Fq "$shared_command" "$contributing" || {
    echo "CONTRIBUTING.md is missing: $shared_command" >&2
    exit 1
  }
done

printf '%s\n' "$audit_job" | grep -Fq 'cargo deny check' || {
  echo 'the CI audit job no longer runs cargo deny check' >&2
  exit 1
}
for documented_command in 'cargo deny check' 'git diff --check'; do
  for validation_document in "$pr_template" "$contributing"; do
    grep -Fq "$documented_command" "$validation_document" || {
      echo "$validation_document is missing: $documented_command" >&2
      exit 1
    }
  done
done

# Pull request titles feed release-please through the squashed commit subject.
pr_title_workflow="$root/.github/workflows/pr-title.yml"
grep -Fq 'amannn/action-semantic-pull-request@' "$pr_title_workflow"
for commit_type in feat fix docs test ci chore refactor; do
  grep -Fqx "            $commit_type" "$pr_title_workflow" || {
    echo "the pull request title check does not accept the '$commit_type' prefix" >&2
    exit 1
  }
  grep -Fq "$commit_type: " "$contributing" || {
    echo "CONTRIBUTING.md no longer documents the '$commit_type' prefix" >&2
    exit 1
  }
done

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
printf '%s\n' "$crate_job" | grep -Fq 'rust-lang/crates-io-auth-action@'
printf '%s\n' "$crate_job" | grep -Fq 'id-token: write'
printf '%s\n' "$crate_job" | grep -Fq 'CARGO_REGISTRY_TOKEN: ${{ steps.auth.outputs.token || secrets.CARGO_REGISTRY_TOKEN }}'
printf '%s\n' "$crate_job" | grep -Fq 'no crates.io credential'
printf '%s\n' "$npm_job" | grep -Fq 'npm view "getdalo@${version}" version'
printf '%s\n' "$npm_job" | grep -Fq 'is already published on npm'
printf '%s\n' "$npm_job" | grep -Fq 'if test -f package-lock.json'
printf '%s\n' "$npm_job" | grep -Fq 'No package-lock.json or dependencies in legacy release; skipping npm install'
printf '%s\n' "$npm_job" | grep -Fq 'package-lock.json is missing for an npm package with dependencies'
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

# dalo.sh is assembled by site/build.mjs at deploy time: the workflow must run
# the build, in order, and upload the assembled tree rather than the sources.
pages_workflow="$root/.github/workflows/pages.yml"
pages_job="$(awk '/^  deploy:/{on=1} on && /^  [A-Za-z0-9_-]+:/ && !/^  deploy:/{exit} on{print}' "$pages_workflow")"
for site_build_command in \
  'corepack pnpm --dir site install --frozen-lockfile' \
  'node site/build.mjs --check' \
  'node site/build.mjs'; do
  printf '%s\n' "$pages_job" | grep -Fqx "          $site_build_command" || {
    echo "the Pages deploy job no longer runs: $site_build_command" >&2
    exit 1
  }
done
printf '%s\n' "$pages_job" | grep -Fqx '          path: ./site/build' || {
  echo 'the Pages deploy job must upload the assembled site/build tree' >&2
  exit 1
}
if printf '%s\n' "$pages_job" | grep -Fq 'Stamp site version'; then
  echo 'the sed-based version stamp is superseded by site/build.mjs; remove it' >&2
  exit 1
fi
build_line="$(printf '%s\n' "$pages_job" | grep -Fn -- '- name: Build site' | cut -d: -f1)"
check_line="$(printf '%s\n' "$pages_job" | grep -Fn 'node site/build.mjs --check' | cut -d: -f1)"
render_line="$(printf '%s\n' "$pages_job" | grep -Fnx '          node site/build.mjs' | cut -d: -f1)"
upload_line="$(printf '%s\n' "$pages_job" | grep -Fn -- '- name: Upload static site' | cut -d: -f1)"
test -n "$build_line" && test -n "$check_line" && test -n "$render_line" && test -n "$upload_line"
if ! { [ "$build_line" -lt "$check_line" ] && [ "$check_line" -lt "$render_line" ] && [ "$render_line" -lt "$upload_line" ]; }; then
  echo 'the Pages deploy job must check, then build, then upload, in that order' >&2
  exit 1
fi

# A stale committed site render must fail the pull request, not the Pages
# deploy on main: CI installs the site dependencies and runs the same check.
site_job="$(ci_job_body site)"
for site_check_command in \
  'corepack pnpm --dir site install --frozen-lockfile' \
  'node site/build.mjs --check'; do
  printf '%s\n' "$site_job" | grep -Fqx "          $site_check_command" || {
    echo "the CI site job no longer runs: $site_check_command" >&2
    exit 1
  }
done
printf '%s\n' "$site_job" | grep -Fq 'COREPACK_ENABLE_DOWNLOAD_PROMPT: "0"' || {
  echo 'the CI site job must disable the corepack download prompt' >&2
  exit 1
}
for validation_document in "$pr_template" "$contributing"; do
  grep -Fq 'node site/build.mjs --check' "$validation_document" || {
    echo "$validation_document is missing: node site/build.mjs --check" >&2
    exit 1
  }
done

echo "workflow checks passed"

#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

for document in "$root/README.md" "$root/site/index.html" "$root/site/install.md" "$root/docs/uninstall.md"; do
  grep -q 'npx getdalo' "$document"
done
for document in "$root/README.md" "$root/site/index.html" "$root/site/install.md"; do
  grep -q 'brew install sebastian-software/tap/dalo' "$document"
  grep -q 'dalo source select sebastian pr-review' "$document"
  grep -q 'dalo approve skill sebastian:pr-review' "$document"
done
grep -q 'dalo audit sebastian:pr-review --agent auto' "$root/README.md"
! grep -R -q 'github-pr-auto-review' "$root/README.md" "$root/site"
grep -q 'brew uninstall dalo' "$root/docs/uninstall.md"
grep -q 'dalo resolve remove-owned <target>:<slot>' "$root/docs/uninstall.md"
grep -q 'resolve list.*exact owned IDs' "$root/docs/uninstall.md"
grep -q 'data-install-method="homebrew"' "$root/site/index.html"
grep -q 'data-install-method="standalone"' "$root/site/index.html"
grep -q 'preferredInstallMethod' "$root/site/main.js"
grep -q 'navigator.maxTouchPoints > 1' "$root/site/main.js"
grep -q '\[data-install-picker\] \[data-copy-target\]' "$root/site/main.js"
grep -q '\.install-methods:not(\[hidden\])' "$root/site/styles.css"
grep -q 'npm uninstall --global getdalo' "$root/docs/uninstall.md"
grep -q 'dalo approve skill' "$root/docs/troubleshooting.md"
grep -q 'source_provenance_mismatch' "$root/docs/troubleshooting.md"
grep -q 'SourceProvenance' "$root/docs/reference.md"
grep -q 'blocking or failed security audits' "$root/docs/reference.md"
grep -q 'blocked materialization operations' "$root/docs/reference.md"
grep -q 'SyncReport.degraded_sources\[\]' "$root/docs/reference.md"
grep -q 'resolution.*, `degraded_sources\[\]`' "$root/docs/reference.md"
grep -q '`no_op`' "$root/docs/reference.md"
grep -q '`dropped_foreign_symlink`' "$root/docs/reference.md"
grep -q '`legacy_bare_approval`' "$root/docs/troubleshooting.md"
grep -q 'dalo approve skill <source-id>:<skill>' "$root/docs/troubleshooting.md"
grep -q '`source_store_debris`' "$root/docs/troubleshooting.md"
grep -q '`skipped_symlink`' "$root/docs/troubleshooting.md"
grep -q 'security audit blocked' "$root/docs/troubleshooting.md"
grep -q -- '--refresh-audit' "$root/docs/reference.md"
grep -q 'audits\[\]' "$root/docs/reference.md"
grep -q 'security-audit block' "$root/docs/ci.md"
grep -q 'dalo approve skill' "$root/docs/getting-started.md"
grep -q 'dalo approve skill' "$root/site/index.html"
grep -q 'dalo team catalog add' "$root/site/index.html"
grep -q 'dalo team catalog update marketing --from main' "$root/README.md"
grep -q 'TeamCatalogUpdateReport' "$root/docs/reference.md"
grep -q '"adoption": AdoptReport' "$root/docs/reference.md"
grep -q '"approval": ApprovalReport' "$root/docs/reference.md"
grep -q 'prints only the blocking `AuditReport`' "$root/docs/reference.md"
grep -q '+copywriting' "$root/site/index.html"
grep -q 'skills = \[\]' "$root/site/index.html"
grep -q 'dalo source add-catalog public' "$root/docs/getting-started.md"
grep -q 'git -C "\$TEAM_REPO" -c commit.gpgSign=false' "$root/docs/getting-started.md"
grep -q 'git -C "\$CATALOG_REPO" -c commit.gpgSign=false' "$root/docs/getting-started.md"
grep -q 'dalo target link generic "\$RUNNER_TEMP/dalo-skills"' "$root/docs/ci.md"
grep -q 'sh tests/docs.sh' "$root/CONTRIBUTING.md"
grep -q 'latest released minor line' "$root/SECURITY.md"
! grep -q '| `0\.4\.x`' "$root/SECURITY.md"
grep -q '__DALO_LASTMOD__' "$root/site/sitemap.xml"
grep -q 'dalo-quickstart.mp4' "$root/site/index.html"
grep -q 'type="video/mp4"' "$root/site/index.html"
grep -q 'dalo-quickstart.mp4' "$root/README.md"
grep -q 'DALO_VERSION' "$root/site/install.md"
grep -q '^## Manual Release Archives' "$root/site/install.md"
grep -q 'shasum -a 256 -c' "$root/site/install.md"
grep -q '^## Shell Completions and Man Page' "$root/site/install.md"
grep -q 'dalo completions <bash|zsh|fish>' "$root/site/install.md"
grep -q '^## Upgrades and Removal' "$root/site/install.md"
grep -q 'source add <id> <git-url-or-path>' "$root/docs/reference.md"
grep -q 'source add-catalog <id> <git-url-or-path>' "$root/docs/reference.md"
grep -q '`version:` entry from the first five lines' "$root/docs/reference.md"
grep -q '`topics:` or `tags:` metadata from the first eight lines' "$root/docs/reference.md"
! sed -n '/MSRV, dependency-audit, and coverage jobs additionally run:/,/^```$/p' "$root/CONTRIBUTING.md" | grep -q 'cargo build --release'
grep -q 'DALO_LINUX_LIBC' "$root/npm/README.md"
grep -q 'DALO_UPDATE_CHECK=never' "$root/README.md"
grep -q 'github:sebastian-software/dalo' "$root/site/install.md"
! grep -q 'One-time bootstrap publish' "$root/npm/README.md"

test_root="$(mktemp -d "${TMPDIR:-/tmp}/dalo-docs-test.XXXXXX")"

cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT INT TERM

store="$test_root/store"
target="$test_root/skills"
source="$test_root/source"
catalog="$test_root/catalog"
mkdir -p "$source/skills/review"
printf '# Review\n' > "$source/skills/review/SKILL.md"
git -C "$source" init -q
git -C "$source" add .
git -C "$source" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -qm initial
mkdir -p "$catalog/skills/review-helper"
printf '# Review Helper\n' > "$catalog/skills/review-helper/SKILL.md"
git -C "$catalog" init -q
git -C "$catalog" add .
git -C "$catalog" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -qm initial

cargo build --quiet
dalo="$root/target/debug/dalo"
"$dalo" --store "$store" init
"$dalo" --store "$store" target link generic "$target"
(
  cd "$source"
  "$dalo" --store "$store" source add project .
)
"$dalo" --store "$store" sync
"$dalo" --store "$store" status --check --json > /dev/null
"$dalo" --store "$store" doctor --check --json > /dev/null
"$dalo" --store "$store" source add-catalog public "$catalog"
"$dalo" --store "$store" source inspect public > /dev/null
"$dalo" --store "$store" source select public review-helper
"$dalo" --store "$store" status > "$test_root/status"
grep -q 'dalo approve skill public:review-helper' "$test_root/status"
"$dalo" --store "$store" approve skill public:review-helper
"$dalo" --store "$store" sync
test -L "$target/review-helper"
"$dalo" source refresh --help | grep -q 'Exit non-zero when selected skills drifted upstream'

echo "documentation checks passed"

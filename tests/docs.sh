#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

# `! command` is exempt from `set -e`, so every negative assertion runs through
# this helper and stops the script when the forbidden content is present.
refute() {
  reason="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "$reason" >&2
    exit 1
  fi
}

for document in "$root/README.md" "$root/site/index.html" "$root/site/install.md" "$root/docs/uninstall.md"; do
  grep -q 'npx getdalo' "$document"
done
for document in "$root/README.md" "$root/site/index.html" "$root/site/install.md"; do
  grep -q 'brew install sebastian-software/tap/dalo' "$document"
  grep -q 'dalo source select sebastian pr-review' "$document"
  grep -q 'dalo approve skill sebastian:pr-review' "$document"
done
grep -q 'dalo audit sebastian:pr-review --reviewer auto' "$root/README.md"
# The security overview is the single page an evaluator is pointed at, so it has
# to exist and stay reachable from the README and the reporting policy.
test -f "$root/docs/security.md"
grep -q '(docs/security.md)' "$root/README.md"
grep -q '(docs/security.md)' "$root/SECURITY.md"
grep -q '(security.md)' "$root/docs/troubleshooting.md"
grep -q '## What Dalo does not protect against' "$root/docs/security.md"
grep -q 'Watch the 15-second demo' "$root/README.md"
refute 'README.md still advertises the 20-second demo' \
  grep -q '20-second demo' "$root/README.md"
grep -q '15-second secure-sync demo' "$root/site/index.html"
refute 'the retired github-pr-auto-review example is still referenced' \
  grep -R -q --exclude-dir=node_modules --exclude-dir=build 'github-pr-auto-review' "$root/README.md" "$root/site"
# The V1 status snapshot is frozen: it lives in the archive, it says so at the
# top, and its "Still Planned" list is epic #836 instead of unowned prose. A
# copy back under docs/rfcs/ would resurrect a second, stale roadmap.
refute 'the V1 status snapshot is back in docs/rfcs/' \
  test -f "$root/docs/rfcs/v1-implementation-status.md"
test -f "$root/docs/archive/v1-implementation-status.md"
grep -q '^\*\*Archived\.\*\*' "$root/docs/archive/v1-implementation-status.md"
grep -q 'issues/836' "$root/docs/archive/v1-implementation-status.md"
grep -q '(v1-implementation-status.md)' "$root/docs/archive/README.md"
# A usage question has exactly one destination, named the same way by the
# support policy and the issue-template chooser. The Question form stays because
# the org standards seed it, so it has to redirect rather than compete.
refute 'SUPPORT.md still hedges about Discussions being enabled' \
  grep -q 'when it is enabled' "$root/SUPPORT.md"
for document in "$root/SUPPORT.md" "$root/.github/ISSUE_TEMPLATE/config.yml"; do
  grep -q 'discussions/categories/q-a' "$document"
  grep -q 'discussions/categories/ideas' "$document"
done
grep -q 'discussions/categories/q-a' "$root/.github/ISSUE_TEMPLATE/question.yml"
grep -q 'fallback' "$root/.github/ISSUE_TEMPLATE/question.yml"
grep -q 'brew uninstall dalo' "$root/docs/uninstall.md"
grep -q 'dalo resolve remove-owned <target>:<slot>' "$root/docs/uninstall.md"
grep -q 'resolve list.*exact owned IDs' "$root/docs/uninstall.md"
grep -q '^## 4. Disable Autosync$' "$root/docs/uninstall.md"
awk '
  /^## 4\. Disable Autosync$/ { autosync_section = 1; next }
  /^## / { autosync_section = 0 }
  autosync_section && $0 == "dalo autosync status" {
    default_status_count++
    default_status_line = NR
  }
  autosync_section && $0 == "dalo autosync uninstall" {
    default_uninstall_count++
    default_uninstall_line = NR
  }
  autosync_section && $0 == "dalo --store <store-path> autosync status" {
    custom_status_count++
    custom_status_line = NR
  }
  autosync_section && $0 == "dalo --store <store-path> autosync uninstall" {
    custom_uninstall_count++
    custom_uninstall_line = NR
  }
  /^## 5\. Remove the Store$/ { store_line = NR }
  END {
    exit !(default_status_count == 1 && default_uninstall_count == 1 \
      && custom_status_count == 1 && custom_uninstall_count == 1 && store_line \
      && default_status_line < default_uninstall_line \
      && default_uninstall_line < custom_status_line \
      && custom_status_line < custom_uninstall_line \
      && custom_uninstall_line < store_line)
  }
' "$root/docs/uninstall.md"
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
grep -Fq 'Git availability' "$root/docs/reference.md"
refute 'reference.md still promises a GitHub CLI doctor check' \
  grep -Fq 'GitHub CLI' "$root/docs/reference.md"
grep -Fq 'Git availability' "$root/site/index.html"
refute 'site/index.html still promises a Git auth doctor check' \
  grep -Fq 'Git auth' "$root/site/index.html"
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
# Project-scoped agent folders ship as a documented recipe in 1.0; the FAQ entry
# and the matrix section must both stay reachable and keep pointing at the
# roadmap issue that tracks the first-class target.
grep -Fq "### Can Dalo manage my repository's \`.claude/skills\`?" \
  "$root/docs/troubleshooting.md"
for document in "$root/docs/troubleshooting.md" "$root/docs/agents.md"; do
  grep -Fq 'https://github.com/sebastian-software/dalo/issues/851' "$document" ||
    {
      echo "$document no longer links the project-scoped target issue #851" >&2
      exit 1
    }
done
grep -q '^## Project-scoped folders$' "$root/docs/agents.md"
grep -Fq '(troubleshooting.md#can-dalo-manage-my-repositorys-claudeskills)' \
  "$root/docs/agents.md"
grep -q -- '--refresh-audit' "$root/docs/reference.md"
grep -q 'audits\[\]' "$root/docs/reference.md"
grep -q 'security-audit block' "$root/docs/ci.md"
grep -q 'dalo approve skill' "$root/docs/getting-started.md"
grep -q 'dalo approve skill' "$root/site/index.html"
grep -q 'dalo team catalog add' "$root/site/index.html"
# The worked pin-advance example lives in the team guide; the README only
# points at it, so the exact command is asserted where it is now documented.
grep -q 'dalo team catalog update marketing --from main' "$root/docs/team.md"
grep -q 'TeamCatalogUpdateReport' "$root/docs/reference.md"
grep -q '"adoption": AdoptReport' "$root/docs/reference.md"
grep -q '"approval": ApprovalReport' "$root/docs/reference.md"
grep -q 'prints only the blocking `AuditReport`' "$root/docs/reference.md"
grep -q '+copywriting' "$root/site/index.html"
grep -q 'skills = \[\]' "$root/site/index.html"
grep -q 'dalo source add-catalog public' "$root/docs/getting-started.md"
grep -q 'git -C "\$TEAM_REPO" -c commit.gpgSign=false' "$root/docs/getting-started.md"
grep -q 'git -C "\$CATALOG_REPO" -c commit.gpgSign=false' "$root/docs/getting-started.md"
# Both onboarding pages exist, each covers its own persona, and they link to
# each other so neither audience lands on the wrong one.
test -f "$root/docs/getting-started.md"
test -f "$root/docs/team.md"
grep -q 'dalo target detect' "$root/docs/getting-started.md"
grep -q 'dalo next' "$root/docs/getting-started.md"
grep -Fq '(team.md)' "$root/docs/getting-started.md"
grep -q 'dalo team init' "$root/docs/team.md"
grep -q 'dalo team catalog update' "$root/docs/team.md"
grep -Fq '(getting-started.md)' "$root/docs/team.md"
grep -Fq '[Team repository guide](docs/team.md)' "$root/README.md"
grep -q 'dalo target link generic "\$RUNNER_TEMP/dalo-skills"' "$root/docs/ci.md"

# The README leads with the five-minute path: a first-time reader must reach a
# working `dalo sync` in the first screen, and every step of that path stays
# copy-pasteable.
for quickstart_command in \
  'dalo init' \
  'dalo target detect' \
  'dalo target link codex' \
  'dalo source add company git@github.com:acme/agent-skills.git' \
  'dalo sync' \
  'dalo status'; do
  grep -Fq "$quickstart_command" "$root/README.md" \
    || { echo "the README five-minute path no longer runs: $quickstart_command" >&2; exit 1; }
done
readme_first_screen="$(head -n 100 "$root/README.md")"
printf '%s\n' "$readme_first_screen" | grep -Fxq 'dalo sync' \
  || { echo 'the README no longer reaches `dalo sync` within its first screen' >&2; exit 1; }
# Recovery guidance stays in the README: one worked example whose output names
# the next command, plus the pointer to the full finding list.
grep -Fq 'pending approval: sebastian:pr-review (run: dalo approve skill sebastian:pr-review)' "$root/README.md"
grep -Fq '(docs/troubleshooting.md)' "$root/README.md"

# Portable plugins, tools, and hooks are reference-grade material: one link away
# from the README, never inline in it.
test -f "$root/docs/plugins.md"
grep -Fq '[Plugins, tools, and hooks](docs/plugins.md)' "$root/README.md"
for plugin_topic in 'PLUGIN.toml' '[[tool]]' '[[hook]]' 'dalo plugin validate' \
  'dalo plugin review' 'dalo approve tool' 'dalo approve hook' 'plugins/state.json'; do
  grep -Fq "$plugin_topic" "$root/docs/plugins.md" \
    || { echo "docs/plugins.md no longer documents $plugin_topic" >&2; exit 1; }
  refute "the README inlines the plugin reference topic $plugin_topic again" \
    grep -Fq "$plugin_topic" "$root/README.md"
done
grep -q 'sh tests/docs.sh' "$root/CONTRIBUTING.md"

# The 1.0 compatibility contract is a constraint, not prose: the page must
# exist, name every persisted file it promises to cover, keep the three tiers
# and the change policy, and carry the library-API statement word for word --
# the same sentence that has to be in the lib.rs crate documentation, so
# docs.rs and dalo.sh cannot disagree.
compatibility="$root/docs/compatibility.md"
test -f "$compatibility"
for persisted_file in config.toml state.toml lock.toml approvals.toml source-lock.toml dalo.toml PLUGIN.toml; do
  grep -Fq "\`$persisted_file\`" "$compatibility" \
    || { echo "docs/compatibility.md does not name the persisted file $persisted_file" >&2; exit 1; }
done
for compatibility_section in \
  '## Tier 1: Stable in 1.x' \
  '## Tier 2: Experimental' \
  '## Tier 3: Not covered' \
  '## Change policy' \
  '## Designed scale and performance envelope' \
  '## Supported platforms' \
  '## Support window'; do
  grep -Fq "$compatibility_section" "$compatibility" \
    || { echo "docs/compatibility.md is missing the section: $compatibility_section" >&2; exit 1; }
done
for exit_code in '`0`' '`1`' '`2`' '`3`' '`4`'; do
  grep -Fq "| $exit_code |" "$compatibility"
done
grep -Fq 'Windows is supported through WSL only' "$compatibility"
# The published performance numbers have to stay an envelope, stay attributed to
# the hardware they were taken on, and stay reproducible by the test that
# produced them -- otherwise they read as a promise nobody measured.
grep -Fq 'These numbers are an envelope, not a promise' "$compatibility" \
  || { echo 'docs/compatibility.md no longer frames the numbers as an envelope' >&2; exit 1; }
grep -Fq 'Apple M1 Ultra' "$compatibility" \
  || { echo 'docs/compatibility.md no longer names the measurement hardware' >&2; exit 1; }
envelope_command='cargo test --release --locked --test performance -- --ignored --nocapture'
grep -Fq "$envelope_command" "$compatibility" \
  || { echo 'docs/compatibility.md no longer shows how to reproduce the envelope' >&2; exit 1; }
test -f "$root/tests/performance.rs" \
  || { echo 'the performance measurement and smoke test are missing' >&2; exit 1; }
library_stance='The Rust library API is not a semver contract; the CLI, its exit codes,'
grep -Fq "$library_stance" "$compatibility" \
  || { echo 'docs/compatibility.md no longer states the library API stance' >&2; exit 1; }
grep -Fq "$library_stance" "$root/src/lib.rs" \
  || { echo 'src/lib.rs crate documentation no longer states the library API stance' >&2; exit 1; }
# The statement belongs in the crate documentation, before the module list, so
# docs.rs shows it in the first paragraph.
lib_crate_docs="$(awk '/^#!\[/ { exit } { print }' "$root/src/lib.rs")"
printf '%s\n' "$lib_crate_docs" | grep -Fq "$library_stance" \
  || { echo 'the library API stance left the src/lib.rs crate documentation header' >&2; exit 1; }
# Pure CLI plumbing stays out of the rendered library documentation.
for hidden_module in cli term update; do
  grep -B 1 -Fx "pub mod $hidden_module;" "$root/src/lib.rs" | grep -Fq '#[doc(hidden)]' \
    || { echo "src/lib.rs no longer hides the CLI plumbing module $hidden_module" >&2; exit 1; }
done
grep -Fq 'docs/compatibility.md' "$root/SECURITY.md"
grep -Fq 'docs/compatibility.md' "$root/README.md.src"
grep -Fq 'compatibility.md' "$root/docs/reference.md"
grep -Fq '0008-compatibility-contract.md' "$root/docs/adr/README.md"
test -f "$root/docs/adr/0008-compatibility-contract.md"

# The upgrading guide is the page a 0.x user is sent to, so it has to exist, be
# rendered, link the compatibility contract and the security overview, and name
# every spelling 1.0 removed together with its replacement.
#
# MAINTENANCE: this list is the breaking-change inventory for the 1.0 line. The
# CHANGELOG cannot be the source here, because 0.16.0 is not released and the
# `!:` commits that carry these removals have no released tag between them yet.
# Whenever another `!:` commit lands before 1.0, add the removed spelling to
# this list and document it in docs/upgrading.md in the same pull request.
upgrading="$root/docs/upgrading.md"
test -f "$upgrading"
# The curated 1.0 release body is committed so a maintainer can paste it over the
# generated release notes; it repeats the same inventory, so both are checked.
release_notes="$root/.github/release-notes/1.0.0.md"
test -f "$release_notes"
for breaking_document in "$upgrading" "$release_notes"; do
  for removed_spelling in '`--yes`' 'audit --agent <reviewer>' 'select <id> --unselect' '`--refresh`' 'target ID `cursor`'; do
    grep -Fq -e "$removed_spelling" "$breaking_document" \
      || { echo "$breaking_document does not name the removed spelling $removed_spelling" >&2; exit 1; }
  done
  for upgrading_replacement in \
    'audit --reviewer <reviewer>' \
    'source unselect <id> <skill>...' \
    '`--refresh-audit`' \
    'dalo target link generic ~/.cursor/skills'; do
    grep -Fq -e "$upgrading_replacement" "$breaking_document" \
      || { echo "$breaking_document does not name the replacement $upgrading_replacement" >&2; exit 1; }
  done
done
grep -Fq '(compatibility.md)' "$upgrading" \
  || { echo 'docs/upgrading.md no longer links the compatibility contract' >&2; exit 1; }
grep -Fq '(security.md)' "$upgrading" \
  || { echo 'docs/upgrading.md no longer links the security overview' >&2; exit 1; }
grep -Fq 'schema_migration_pending' "$upgrading"
# The upgrading guide opens with the deep link to the 1.0.0 release page. The tag
# format is `dalo-v<version>`, so the link only resolves once 1.0.0 is published.
grep -Fq 'https://github.com/sebastian-software/dalo/releases/tag/dalo-v1.0.0' \
  "$upgrading"
# The curated release body has to keep linking the three pages it sends a reader
# to, and docs/ci.md has to keep documenting how the file reaches the release.
for release_notes_link in compatibility security upgrading; do
  grep -Fq "https://github.com/sebastian-software/dalo/blob/main/docs/$release_notes_link.md" \
    "$release_notes" \
    || { echo "the 1.0 release notes no longer link docs/$release_notes_link.md" >&2; exit 1; }
done
grep -Fq '.github/release-notes/1.0.0.md' "$root/docs/ci.md" \
  || { echo 'docs/ci.md no longer documents the curated release body' >&2; exit 1; }
# Reachable from the contract it demonstrates, the README, and the FAQ.
grep -Fq '(upgrading.md)' "$compatibility"
grep -Fq '(docs/upgrading.md)' "$root/README.md"
grep -Fq '(docs/upgrading.md)' "$root/README.md.src"
grep -Fq '(upgrading.md)' "$root/docs/troubleshooting.md"
grep -Fq 'I upgraded and doctor reports `schema_migration_pending`' \
  "$root/docs/troubleshooting.md"

grep -q 'latest release on the default branch' "$root/SECURITY.md"
# Both private channels must stay named. GitHub private vulnerability reporting
# is enabled on the repository, and it is the channel a reporter finds first.
grep -q 'GitHub private vulnerability reporting' "$root/SECURITY.md"
grep -q 'Report a vulnerability' "$root/SECURITY.md"
grep -q 'security@sebastian-software.de' "$root/SECURITY.md"
refute 'SECURITY.md still lists the 0.4.x line as supported' \
  grep -q '| `0\.4\.x`' "$root/SECURITY.md"
grep -q '__DALO_LASTMOD__' "$root/site/sitemap.xml"

# The documentation published on dalo.sh is rendered from docs/*.md by
# site/build.mjs and committed, so it must exist, carry the site styles, and be
# reachable from the sitemap, the documentation index, and the footer.
for document in getting-started team reference compatibility upgrading plugins agents ci troubleshooting uninstall comparison; do
  page="$root/site/docs/$document.html"
  test -f "$page"
  title="$(sed -n 's/^# //p' "$root/docs/$document.md" | head -n 1)"
  grep -Fq "<title>$title · Dalo documentation</title>" "$page"
  grep -Fq '<link rel="stylesheet" href="/styles.css" />' "$page"
  grep -Fq '<link rel="stylesheet" href="/docs.css" />' "$page"
  grep -Fq "https://github.com/sebastian-software/dalo/blob/main/docs/$document.md" "$page"
  grep -Fq "https://dalo.sh/docs/$document.html" "$root/site/sitemap.xml"
  grep -Fq "/docs/$document.html" "$root/site/docs/index.html"
done
test -f "$root/site/docs/index.html"
grep -Fq 'https://dalo.sh/docs/' "$root/site/sitemap.xml"
grep -Fq '<a href="/docs/">All documentation</a>' "$root/site/index.html"
# The competitor comparison is dated, linked from the homepage and the README,
# and its homepage summary points at the full page.
grep -Fq 'Snapshot from September 2026' "$root/site/index.html"
grep -Fq 'href="/docs/comparison.html"' "$root/site/index.html"
grep -Fq 'https://dalo.sh/docs/comparison.html' "$root/README.md"
grep -Fq 'This page is a snapshot from' "$root/docs/comparison.md"
grep -Fq '<a href="/docs/reference.html">Reference</a>' "$root/site/index.html"
refute 'the site still links its documentation as repository blobs' \
  grep -q 'blob/main/docs/' "$root/site/index.html"

# The experimental package specification has an explicit version route and a
# landing page. Its renderer also needs source-relative links: the nested spec
# source links into docs/adr, docs/rfcs, and tests without dropping path levels.
test -f "$root/site/spec/index.html"
test -f "$root/site/spec/0.1/index.html"
test -f "$root/site/spec/0.1/compatibility.html"
test -f "$root/site/spec/0.1/plugin-v1.schema.json"
cmp "$root/docs/spec/plugin-v1.schema.json" "$root/site/spec/0.1/plugin-v1.schema.json"
grep -Fq 'https://dalo.sh/spec/' "$root/site/sitemap.xml"
grep -Fq 'https://dalo.sh/spec/0.1/' "$root/site/sitemap.xml"
grep -Fq 'https://dalo.sh/spec/0.1/compatibility.html' "$root/site/sitemap.xml"
grep -Fq 'https://dalo.sh/spec/0.1/plugin-v1.schema.json' "$root/site/sitemap.xml"
grep -Fq 'href="/spec/0.1/"' "$root/site/spec/index.html"
grep -Fq 'href="/spec/0.1/compatibility.html"' "$root/site/spec/index.html"
grep -Fq 'href="/spec/0.1/plugin-v1.schema.json"' "$root/site/spec/0.1/index.html"
grep -Fq 'docs/adr/0006-passive-portable-plugins.md' "$root/site/spec/0.1/index.html"
grep -Fq 'docs/rfcs/0005-portable-plugins-and-agent-stacks.md' "$root/site/spec/0.1/index.html"
grep -Fq 'tests/fixtures/upstream-hooks/README.md' "$root/site/spec/0.1/index.html"
grep -Fq 'href="/docs/reference.html"' "$root/site/spec/0.1/index.html"
grep -Fq 'href="/spec/0.1/"' "$root/site/spec/0.1/compatibility.html"
grep -Fq 'href="/spec/0.1/plugin-v1.schema.json"' "$root/site/spec/0.1/compatibility.html"
grep -Fq 'href="/spec/"' "$root/site/index.html"
grep -Fq '>Product</a>' "$root/site/index.html"
grep -Fq '>Docs</a>' "$root/site/index.html"
grep -Fq '>Spec</a>' "$root/site/index.html"
grep -Fq '>GitHub</a>' "$root/site/index.html"

# The checked-in landing page shows a real version, never a deploy placeholder.
refute 'site/index.html still carries the deploy-time version placeholder' \
  grep -q '__DALO_VERSION__' "$root/site/index.html"
test "$(grep -c 'data-dalo-version' "$root/site/index.html")" -eq 2
grep -Eq '<span data-dalo-version>[0-9]+\.[0-9]+\.[0-9]+</span>' "$root/site/index.html"
grep -Eq '"softwareVersion": "[0-9]+\.[0-9]+\.[0-9]+"' "$root/site/index.html"
grep -Fq 'x-release-please-version' "$root/site/index.html"
grep -Fq 'x-release-please-start-version' "$root/site/index.html"

# The hero transcript is the output the current CLI prints, not the pre-0.14 one.
grep -Fq 'target[generic]:/review -&gt; store:/local/skills/review' "$root/site/index.html"
grep -Fq 'synced: 1 skill across 2 targets (2 created)' "$root/site/index.html"
grep -Fq 'security preflight: deterministic checks only' "$root/site/index.html"
grep -Fq 'target[generic]:/review -> store:/local/skills/review' "$root/video/src/QuickstartVideo.tsx"
refute 'the quickstart video source still uses the pre-0.14 absolute sync path' \
  grep -Fq 'applied  create     /tmp/dalo/skills/review -> /tmp/dalo/store/local/skills/review' "$root/video/src/QuickstartVideo.tsx"
grep -Fq 'synced: 1 skill across 1 target (1 created)' "$root/video/src/QuickstartVideo.tsx"
refute 'the hero terminal still shows the pre-0.14 sync output' \
  grep -q 'skills/review -&gt; /tmp/dalo/store' "$root/site/index.html"

# When the site dependencies are installed, the committed render must be current.
if [ -d "$root/site/node_modules" ]; then
  node "$root/site/build.mjs" --check
fi
grep -q 'dalo-quickstart.mp4' "$root/site/index.html"
grep -q 'type="video/mp4"' "$root/site/index.html"
grep -q 'dalo-quickstart.mp4' "$root/README.md"
grep -q 'Get it wrong. Dalo gets you back.' "$root/site/index.html"
grep -q 'dalo synk' "$root/site/index.html"
grep -q "a similar subcommand exists: 'sync'" "$root/site/index.html"
grep -q "error: skill 'company:relese-helper' was not found; known skills: company:new-skill, company:release-helper" "$root/site/index.html"
grep -q 'pending approval: sebastian:tech-docs (run: dalo approve skill sebastian:tech-docs)' "$root/site/index.html"
grep -q 'Recover without googling.' "$root/README.md"
grep -q 'Security preflight and review gate' "$root/site/index.html"
grep -q 'dalo audit sebastian:pr-review' "$root/site/index.html"
grep -q 'security audits and review gates' "$root/site/index.html"
grep -q 'security preflight: deterministic checks and compatible cached findings only; sync did not run an agent reviewer; passing is not a safety guarantee' "$root/site/index.html"
grep -q 'durationInFrames={450}' "$root/video/src/Root.tsx"
refute 'the site requests a CDN-hosted player instead of self-hosted assets' \
  grep -R -q -E --exclude-dir=node_modules --exclude-dir=build 'cdn\.jsdelivr\.net|AsciinemaPlayer|asciinema-player' "$root/site"
grep -q 'DALO_VERSION' "$root/site/install.md"
grep -q 'dalo-v<version>.*, `v<version>`.*, or `<version>`' "$root/site/install.md"
grep -q '`<version>`, `v<version>`, or `dalo-v<version>`' "$root/npm/README.md"
refute 'the install documents still pin retired example versions' \
  grep -q -E 'dalo-v0\.6\.1|v0\.7\.0|dalo-v0\.7\.0' "$root/site/install.md" "$root/npm/README.md"
grep -q '^## Manual Release Archives' "$root/site/install.md"
grep -q 'shasum -a 256 -c' "$root/site/install.md"
grep -q '^## Shell Completions and Man Page' "$root/site/install.md"
grep -q 'dalo completions <bash|zsh|fish>' "$root/site/install.md"
grep -q '^## Upgrades and Removal' "$root/site/install.md"
grep -q 'source add <id> <git-url-or-path>' "$root/docs/reference.md"
grep -q 'source add-catalog <id> <git-url-or-path>' "$root/docs/reference.md"
delivery_sandbox_reference="$(awk '/^Generator execution is fail-closed/{on=1} on && /^Generated delivery has two independent approvals:/{exit} on{print}' "$root/docs/reference.md")"
printf '%s\n' "$delivery_sandbox_reference" | grep -Fq 'Landlock ABI v4'
printf '%s\n' "$delivery_sandbox_reference" | grep -Fq 'denies TCP connection and bind operations'
printf '%s\n' "$delivery_sandbox_reference" | grep -Fq 'not a general read-confinement boundary'
printf '%s\n' "$delivery_sandbox_reference" | grep -Fq 'do not cover UDP or UNIX-domain sockets'
case "$delivery_sandbox_reference" in
  *'Landlock ABI v3'*)
    echo 'generated-delivery docs still claim the weaker Landlock ABI v3 boundary' >&2
    exit 1
    ;;
esac
source_select_reference="$(awk '/^### `dalo source select <id> <skill>\.\.\.`$/{on=1;next} on && /^### /{exit} on{print}' "$root/docs/reference.md")"
source_select_page="$(awk '/id="dalo-source-select-id-skill"/{on=1;next} on && /<h3 /{exit} on{print}' "$root/site/docs/reference.html")"
for source_select_document in "$source_select_reference" "$source_select_page"; do
  printf '%s\n' "$source_select_document" | grep -Fq 'stable frontmatter ID, slot name, catalog-relative path'
  printf '%s\n' "$source_select_document" | grep -Fq 'source-qualified'
done
grep -Fq '| `sources[].selection` | Persisted catalog selections by stable ID, slot name, or catalog-relative path. Source-qualified command input is normalized before storage. Empty for local/team sources. |' "$root/docs/reference.md"
source_selection_field="$(grep -F -A1 '<td><code>sources[].selection</code></td>' "$root/site/docs/reference.html")"
printf '%s\n' "$source_selection_field" | grep -Fq 'Persisted catalog selections by stable ID, slot name, or catalog-relative path. Source-qualified command input is normalized before storage. Empty for local/team sources.'
source_config_selection_rustdoc="$(awk '
  /^    \/\/\/ Persisted selected skill references for a catalog source\./ { on = 1 }
  on { print }
  on && /^    pub selection: Vec<String>/ { exit }
' "$root/src/source.rs")"
printf '%s\n' "$source_config_selection_rustdoc" | grep -Fq 'frontmatter ID, a slot name, or a catalog-relative path.'
printf '%s\n' "$source_config_selection_rustdoc" | grep -Fq 'source-qualified form accepted by `source select` is normalized before'
catalog_select_skills_rustdoc="$(awk '
  /^\/\/\/ Select skills from a catalog\./ { on = 1 }
  on { print }
  on && /^pub fn select_skills/ { exit }
' "$root/src/catalog.rs")"
printf '%s\n' "$catalog_select_skills_rustdoc" | grep -Fq 'ID, slot name, catalog-relative path, or a source-qualified'
printf '%s\n' "$catalog_select_skills_rustdoc" | grep -Fq '<source-id>:<slot-or-stable-id>'
grep -q '`version:` entry from the first five lines' "$root/docs/reference.md"
grep -q '`topics:` or `tags:` metadata from the first eight lines' "$root/docs/reference.md"
if sed -n '/MSRV, dependency-audit, coverage, and site-render jobs additionally run:/,/^```$/p' "$root/CONTRIBUTING.md" \
  | grep -q 'cargo build --release'; then
  echo 'CONTRIBUTING repeats the release build in the extra-jobs command set' >&2
  exit 1
fi
grep -Fq 'cargo build --release --locked --target "$(rustc -vV | sed -n '\''s/^host: //p'\'')"' "$root/CONTRIBUTING.md"
grep -Fq 'cargo +"$msrv" check --locked --all-targets --all-features' "$root/CONTRIBUTING.md"
grep -Fq 'cargo test --locked' "$root/docs/archive/milestones/README.md"
grep -Fq 'cargo clippy --locked --all-targets --all-features -- -D warnings' "$root/docs/archive/milestones/README.md"
refute 'milestone validation policy still uses an unlocked cargo test' \
  grep -Fxq 'cargo test' "$root/docs/archive/milestones/README.md"
refute 'milestone validation policy still uses an unlocked cargo clippy' \
  grep -Fxq 'cargo clippy --all-targets --all-features -- -D warnings' "$root/docs/archive/milestones/README.md"
grep -Fq 'cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines "$(cat coverage-threshold)"' "$root/CONTRIBUTING.md"
grep -Fq '`coverage-threshold` holds the line-coverage gate' "$root/CONTRIBUTING.md"
refute 'CONTRIBUTING.md restates the coverage threshold instead of reading coverage-threshold' \
  grep -Eq 'fail-under-lines[[:space:]]+[0-9]' "$root/CONTRIBUTING.md"
grep -q 'DALO_LINUX_LIBC' "$root/npm/README.md"
grep -Fq 'Release-metadata lookups time out after 30 seconds; archive downloads use a' "$root/npm/README.md"
grep -Fq '10-second response-header timeout and a 120-second body timeout and may be' "$root/npm/README.md"
grep -Fq 'retried up to twice on transient failures (three total attempts)' "$root/npm/README.md"
refute 'npm/README.md still claims archive downloads time out after 30 seconds' \
  grep -Fq 'Release metadata and archive downloads time out after 30 seconds' "$root/npm/README.md"
grep -q 'DALO_UPDATE_CHECK=never' "$root/README.md"
grep -q 'github:sebastian-software/dalo' "$root/site/install.md"
refute 'npm/README.md still documents the one-time bootstrap publish' \
  grep -q 'One-time bootstrap publish' "$root/npm/README.md"
plugin_section="$(awk '/^## `PLUGIN.toml` Portable Plugins, Tools, and Hooks$/{on=1;next} on && /^## /{exit} on{print}' "$root/docs/reference.md")"
tool_section="$(printf '%s\n' "$plugin_section" | awk '/^### Tools$/{on=1;next} on && /^### /{exit} on{print}')"
hook_section="$(printf '%s\n' "$plugin_section" | awk '/^### Hooks$/{on=1;next} on && /^### /{exit} on{print}')"
assert_documented_field() {
  source="$1"
  section="$2"
  field="$3"
  documented_field="${4:-$field}"
  printf '%s\n' "$source" | grep -Eq "^[[:space:]]*(pub )?$field:"
  printf '%s\n' "$section" | grep -Fq "\`$documented_field\`"
}

assert_documented_enum_value() {
  file="$1"
  enum="$2"
  variant="$3"
  value="$4"
  section="${5:-$plugin_section}"
  sed -n "/^pub enum $enum {/,/^}/p" "$root/$file" | grep -Eq "^[[:space:]]*$variant,"
  printf '%s\n' "$section" | grep -Fq "\`$value\`"
}

manifest_tool="$(sed -n '/^struct ManifestTool {/,/^}/p' "$root/src/plugin.rs")"
tool_input="$(sed -n '/^pub struct ToolInput {/,/^}/p' "$root/src/plugin.rs")"
hook_descriptor="$(sed -n '/^pub struct HookDescriptorV1 {/,/^}/p' "$root/src/hook.rs")"
hook_matcher="$(sed -n '/^pub struct HookMatcherV1 {/,/^}/p' "$root/src/hook.rs")"
hook_binding="$(sed -n '/^pub struct HookBindingV1 {/,/^}/p' "$root/src/hook.rs")"
for key in schema_version id entry runtime runtime_version platforms inputs argv files cwd env capabilities availability; do
  assert_documented_field "$manifest_tool" "$tool_section" "$key"
done
assert_documented_field "$tool_input" "$tool_section" name
assert_documented_field "$tool_input" "$tool_section" kind type
assert_documented_field "$tool_input" "$tool_section" required
for key in schema_version id tool subject phase effect requirement timeout_ms failure_policy retry error_visibility matcher bindings blocking_scope fallback; do
  assert_documented_field "$hook_descriptor" "$hook_section" "$key"
done
assert_documented_field "$hook_matcher" "$hook_section" tool_names matcher.tool_names
assert_documented_field "$hook_binding" "$hook_section" input
assert_documented_field "$hook_binding" "$hook_section" field
for key in '[[tool]]' '[[hook]]' matcher bindings; do
  printf '%s\n' "$plugin_section" | grep -Fq "$key"
done
for enum_value in \
  'ToolRuntime Executable executable' 'ToolRuntime Python python' 'ToolRuntime Node node' \
  'ToolPlatform Macos macos' 'ToolPlatform Linux linux' \
  'ToolInputType String string' 'ToolInputType Path path' \
  'ToolInputType Integer integer' 'ToolInputType Boolean boolean' \
  'ToolCwd ToolRoot tool_root' \
  'ToolCapability FilesystemRead filesystem_read' 'ToolCapability FilesystemWrite filesystem_write' \
  'ToolCapability Subprocess subprocess' 'ToolCapability Network network' \
  'ToolAvailability Required required' 'ToolAvailability Optional optional'; do
  set -- $enum_value
  assert_documented_enum_value src/plugin.rs "$1" "$2" "$3" "$tool_section"
done
for enum_value in \
  'HookSubject Session session' 'HookSubject UserPrompt user_prompt' \
  'HookSubject ToolCall tool_call' 'HookSubject Workflow workflow' \
  'HookPhase Before before' 'HookPhase After after' 'HookPhase End end' \
  'HookPhase CompletionAttempt completion_attempt' \
  'HookEffect Observe observe' 'HookEffect AddContext add_context' \
  'HookEffect AllowDeny allow_deny' 'HookEffect RewriteInput rewrite_input' \
  'HookEffect ReplaceOutput replace_output' 'HookEffect ContinueWorkflow continue_workflow' \
  'HookRequirement Required required' 'HookRequirement Optional optional' \
  'HookFailurePolicy FailOpen fail_open' 'HookFailurePolicy FailClosed fail_closed' \
  'HookFailurePolicy Report report' 'HookRetryPolicy Never never' \
  'HookErrorVisibility User user' 'HookErrorVisibility ModelAndUser model_and_user' \
  'HookFallback Omit omit' 'HookBlockingScope MatchedEvent matched_event' \
  'HookEventField SessionId session.id' 'HookEventField SessionCwd session.cwd' \
  'HookEventField SessionPermissionMode session.permission_mode' \
  'HookEventField ActorKind actor.kind' 'HookEventField ActorId actor.id' \
  'HookEventField TranscriptPath transcript.path' \
  'HookEventField SessionEndReason session.end_reason' \
  'HookEventField PromptText prompt.text' 'HookEventField ToolCallId tool.call_id' \
  'HookEventField ToolName tool.name' \
  'HookEventField WorkflowAlreadyContinued workflow.already_continued' \
  'HookEventField WorkflowLastMessage workflow.last_message'; do
  set -- $enum_value
  assert_documented_enum_value src/hook.rs "$1" "$2" "$3" "$hook_section"
done
for source in \
  'src/plugin.rs:struct Manifest' 'src/plugin.rs:struct ManifestPlugin' \
  'src/plugin.rs:struct ManifestMember' 'src/plugin.rs:struct ManifestDependency' \
  'src/plugin.rs:struct ManifestFallback' 'src/plugin.rs:struct ManifestTool' \
  'src/plugin.rs:pub struct ToolInput' 'src/hook.rs:pub struct HookDescriptorV1' \
  'src/hook.rs:pub struct HookMatcherV1' 'src/hook.rs:pub struct HookBindingV1'; do
  file="${source%%:*}"
  marker="${source#*:}"
  grep -B 3 -F "$marker" "$root/$file" | grep -Fq '#[serde(deny_unknown_fields)]'
done
printf '%s\n' "$plugin_section" | grep -Fq 'unknown fields are rejected'

# The gate must reject a source-required field that is absent from its section.
missing_availability="$(printf '%s\n' "$tool_section" | sed '/availability/d')"
if assert_documented_field "$manifest_tool" "$missing_availability" availability; then
  echo 'plugin reference gate accepted a missing availability entry' >&2
  exit 1
fi
missing_network="$(printf '%s\n' "$tool_section" | sed '/network/d')"
if assert_documented_enum_value src/plugin.rs ToolCapability Network network "$missing_network"; then
  echo 'plugin reference gate accepted a missing network capability' >&2
  exit 1
fi

resolver_code='blocked_winner_alternate_available'
resolver_emit="$(sed -n '/for blocked in &blocked_skills {/,/active_skills.sort_by/p' "$root/src/resolver.rs")"
review_codes="$(sed -n '/pub const fn requires_review(self)/,/^    }/p' "$root/src/resolver.rs")"
resolver_reference="$(sed -n '/^Resolution diagnostics use these codes/,/^## Store Layout/p' "$root/docs/reference.md")"
resolver_troubleshooting="$(sed -n '/^### Resolver Diagnostics$/,/^### Required-Closure Block Reasons$/p' "$root/docs/troubleshooting.md")"
assert_blocked_winner_alternate_docs() {
  reference="$1"
  troubleshooting="$2"
  printf '%s\n' "$resolver_emit" | grep -Fq 'for blocked in &blocked_skills' || return 1
  printf '%s\n' "$resolver_emit" | grep -Fq 'approved_alternates' || return 1
  printf '%s\n' "$resolver_emit" | grep -Fq 'BlockedWinnerAlternateAvailable' || return 1
  printf '%s\n' "$resolver_emit" | grep -Fq 'refs.first()' || return 1
  ! printf '%s\n' "$review_codes" | grep -Fq 'Self::BlockedWinnerAlternateAvailable' || return 1
  grep -Fq '"blocked_winner_alternate_available"' "$root/src/resolver.rs" || return 1
  printf '%s\n' "$reference" | grep -Fq "\`$resolver_code\`" || return 1
  printf '%s\n' "$reference" | grep -Fq '`code`' || return 1
  printf '%s\n' "$reference" | grep -Fq '`message`' || return 1
  printf '%s\n' "$reference" | grep -Fq '`source_ref`' || return 1
  printf '%s\n' "$troubleshooting" | grep -Fq "\`$resolver_code\`" || return 1
  printf '%s\n' "$troubleshooting" | grep -Fq 'does not auto-promote the alternate' || return 1
  printf '%s\n' "$troubleshooting" | grep -Fq 'lower `dalo source priority` value' || return 1
}
assert_blocked_winner_alternate_docs "$resolver_reference" "$resolver_troubleshooting"

# The complete reference and recovery row are required, not optional prose.
missing_resolver_reference="$(printf '%s\n' "$resolver_reference" | sed "/$resolver_code/d")"
if assert_blocked_winner_alternate_docs "$missing_resolver_reference" "$resolver_troubleshooting"; then
  echo 'resolver diagnostic reference gate accepted a missing code' >&2
  exit 1
fi

reference_status_section="$(awk '
  /^### `dalo status`$/ { in_section = 1; next }
  in_section && /^### / { exit }
  in_section { print }
' "$root/docs/reference.md")"
reference_agent_section="$(awk '
  /^### `dalo agent list/ { in_section = 1; next }
  in_section && /^### / { exit }
  in_section { print }
' "$root/docs/reference.md")"
reference_tool_section="$(awk '
  /^### `dalo tool list/ { in_section = 1; next }
  in_section && /^### / { exit }
  in_section { print }
' "$root/docs/reference.md")"
reference_hook_section="$(awk '
  /^### `dalo hook list/ { in_section = 1; next }
  in_section && /^### / { exit }
  in_section { print }
' "$root/docs/reference.md")"
reference_tool_contract="$(printf '%s\n' "$reference_tool_section" | tr '\n' ' ' | tr -s '[:space:]' ' ')"
printf '%s\n' "$reference_status_section" | grep -Fq '`--check` exits with code 1'
printf '%s\n' "$reference_status_section" | grep -Fq 'full report on stdout for JSON'
printf '%s\n' "$reference_agent_section" | grep -Fq 'source scan errors because its result is incomplete'
printf '%s\n' "$reference_agent_section" | grep -Fq 'package inventory warnings'
printf '%s\n' "$reference_tool_contract" | grep -Fq 'A failed audit returns a non-zero exit code'
printf '%s\n' "$reference_tool_section" | grep -Fq 'rejected plugin packages produce'
printf '%s\n' "$reference_hook_section" | grep -Fq 'rejected plugin packages produce'

store_paths="$(sed -n '/impl StorePaths/,/^}/p' "$root/src/store.rs")"
store_layout="$(awk '/^## Store Layout/{ in_section = 1; next } in_section && /^## /{ exit } in_section{ print }' "$root/docs/reference.md")"
for path in tools generated hooks plugins; do
  printf '%s\n' "$store_paths" | grep -Fq "root.join(\"$path\")"
  printf '%s\n' "$store_layout" | grep -Fq "\`$path/\`"
done
printf '%s\n' "$store_paths" | grep -Fq 'plugin_state_file: root.join("plugins/state.json")'
printf '%s\n' "$store_layout" | grep -Fq '`plugins/state.json`'
printf '%s\n' "$store_paths" | grep -Fq 'hook_state_file: root.join("hooks/state.json")'
printf '%s\n' "$store_layout" | grep -Fq '`hooks/state.json`'
printf '%s\n' "$store_layout" | grep -Fq 'created lazily, not by `dalo init`'
printf '%s\n' "$store_paths" | grep -Fq 'catalog_advance_file: root.join("catalog-advance.toml")'
printf '%s\n' "$store_layout" | grep -Fq '`catalog-advance.toml`'
printf '%s\n' "$store_paths" | grep -Fq 'catalog_lock_file: root.join(".catalog.lock")'
printf '%s\n' "$store_layout" | grep -Fq '`.catalog.lock`'

target_section="$(awk '/^### `DALO_TARGET`$/{on=1;next} on && /^##|^### /{exit} on{print}' "$root/docs/reference.md")"
reference_document="$(cat "$root/docs/reference.md")"
published_targets="$(awk '/^[[:space:]]*for target in \\/ {targets=1; next} targets {sub(/^[[:space:]]*/, ""); last = $0; sub(/[[:space:]]*\\$/, "", last); sub(/; do$/, "", last); print last; if ($0 ~ /; do$/) exit}' "$root/.github/workflows/publish.yml")"
assert_target_reference() {
  section="$1"
  document="$2"
  grep -Fq 'target="${DALO_TARGET:-$(detect_target)}"' "$root/site/install.sh" || return 1
  grep -Fq '### Installer environment variables' "$root/site/install.md" || return 1
  printf '%s\n' "$document" | grep -Fq 'Installer-only release target override' || return 1
  printf '%s\n' "$section" | grep -Fq 'non-empty value takes precedence' || return 1
  printf '%s\n' "$section" | grep -Fq 'unset or empty value' || return 1
  printf '%s\n' "$document" | grep -Fq '../site/install.md#installer-environment-variables' || return 1
  for target in $published_targets; do
    printf '%s\n' "$section" | grep -Fq "\`$target\`" || return 1
  done
  expected_targets="$(printf '%s\n' $published_targets | sort -u)"
  documented_targets="$(
    printf '%s\n' "$section" \
      | grep -Eo '`[A-Za-z0-9_]+-(unknown-linux-(gnu|musl)|apple-darwin)`' \
      | tr -d '`' \
      | sort -u
  )"
  test "$documented_targets" = "$expected_targets" || return 1
}
assert_target_reference "$target_section" "$reference_document"

# A missing cross-reference or a non-published triplet must fail this gate.
missing_target_document="$(printf '%s\n' "$reference_document" | sed '/site\/install.md/d')"
if assert_target_reference "$target_section" "$missing_target_document"; then
  echo 'DALO_TARGET reference gate accepted a missing installer link' >&2
  exit 1
fi
wrong_target_section="$target_section
Unsupported example: \`powerpc64-unknown-linux-gnu\`."
if assert_target_reference "$wrong_target_section" "$reference_document"; then
  echo 'DALO_TARGET reference gate accepted a wrong published target' >&2
  exit 1
fi

# Keep the published target tables aligned with the built-in registry. The
# expected IDs and default paths come from `src/target.rs` itself, so a target
# cannot be added, removed, or repointed without updating every table a user
# reads. V1 ships only verified targets, so no table may carry an
# "experimental" or "unverified" label.
registry_source="$root/src/target.rs"
registry_ids="$(awk '
  /^pub fn registry\(\)/ { inside = 1 }
  inside && /^}/ { exit }
  inside && /^ *id: "/ {
    line = $0
    sub(/^ *id: "/, "", line)
    sub(/".*$/, "", line)
    print line
  }
' "$registry_source")"
registry_paths="$(awk '
  /^pub fn registry\(\)/ { inside = 1 }
  inside && /^}/ { exit }
  inside && /default_path: Some\("/ {
    line = $0
    sub(/^.*default_path: Some\("/, "", line)
    sub(/"\).*$/, "", line)
    print line
  }
' "$registry_source" | sort -u)"
test -n "$registry_ids"
test -n "$registry_paths"
grep -q '^## Support matrix$' "$root/docs/agents.md"
grep -q '| Verified version | Verified on |' "$root/docs/agents.md"
for target_id in $registry_ids; do
  grep -Fq "| \`$target_id\` |" "$root/docs/reference.md" ||
    {
      echo "target \`$target_id\` is missing from the docs/reference.md target table" >&2
      exit 1
    }
  grep -Fq "| \`$target_id\` |" "$root/docs/agents.md" ||
    {
      echo "target \`$target_id\` is missing from the docs/agents.md support matrix" >&2
      exit 1
    }
done
for target_path in $registry_paths; do
  for document in "$root/README.md" "$root/docs/reference.md" "$root/docs/agents.md" "$root/site/index.html"; do
    grep -Fq "$target_path" "$document" ||
      {
        echo "default target path $target_path is missing from $document" >&2
        exit 1
      }
  done
done
refute 'src/target.rs still registers an experimental target' \
  grep -q 'support: TargetSupport::Experimental' "$registry_source"
refute 'the reference target table still labels a target experimental' \
  grep -q '| experimental |' "$root/docs/reference.md"
refute 'the site targets section still carries an experimental badge' \
  grep -q 'badge-exp' "$root/site/index.html"
refute 'the site targets section still advertises an unverified target path' \
  grep -q 'target-path">unverified' "$root/site/index.html"

test_root="$(mktemp -d "${TMPDIR:-/tmp}/dalo-docs-test.XXXXXX")"

cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT INT TERM

# Keep the actionable tool, hook, plugin-projection, inventory, and owned-link
# doctor rows in the troubleshooting table aligned with the production emitter.
# The expected names come from DoctorCode callsites and its serializer mapping;
# this deliberately avoids a hand-maintained list of code strings.
doctor_source="$root/src/doctor.rs"
doctor_table="$test_root/doctor-findings-table"
doctor_emitted="$test_root/doctor-emitted-variants"
doctor_enum="$test_root/doctor-enum-variants"
doctor_expected="$test_root/doctor-expected-codes"
doctor_documented="$test_root/doctor-documented-codes"

awk '
  /^fn code_name/ { exit }
  { print }
' "$doctor_source" \
  | grep -o 'DoctorCode::[A-Za-z0-9_]*' \
  | sed 's/DoctorCode:://' \
  | grep -E '^(Tool|Hook|PluginProjection|SourceInventoryDegraded|OwnedSymlinkRepointed)' \
  | sort -u > "$doctor_emitted"

awk '
  /^pub enum DoctorCode/ { in_enum = 1; next }
  in_enum && /^}/ { exit }
  in_enum && /^[[:space:]]+[A-Z][A-Za-z0-9_]*,/ {
    line = $0
    sub(/^[[:space:]]+/, "", line)
    sub(/,.*/, "", line)
    print line
  }
' "$doctor_source" | sort -u > "$doctor_enum"

comm -23 "$doctor_emitted" "$doctor_enum" | grep -q '^' && {
  echo "doctor emitter uses a DoctorCode missing from the enum" >&2
  exit 1
}

awk '
  /^fn code_name/ { in_mapping = 1; next }
  in_mapping && /^}/ { exit }
  in_mapping && /DoctorCode::/ {
    line = $0
    sub(/.*DoctorCode::/, "", line)
    split(line, fields, / => "/)
    variant = fields[1]
    code = fields[2]
    sub(/".*/, "", code)
    print variant " " code
  }
' "$doctor_source" \
  | while IFS=' ' read -r variant code; do
      grep -Fx "$variant" "$doctor_emitted" >/dev/null && printf '%s\n' "$code"
    done \
  | sort -u > "$doctor_expected"

awk '
  /^## Doctor Findings/ { in_table = 1; next }
  in_table && /^## / { exit }
  in_table && /^\| `/ {
    line = $0
    sub(/^\| `/, "", line)
    split(line, fields, /`/)
    print fields[1]
  }
' "$root/docs/troubleshooting.md" > "$doctor_table"

grep -E '^(tool_.*|hook_.*|plugin_projection_.*|source_inventory_degraded|owned_symlink_repointed)$' \
  "$doctor_table" | sort > "$doctor_documented"

duplicates="$(sort "$doctor_documented" | uniq -d)"
test -z "$duplicates" || {
  echo "duplicate actionable doctor documentation rows:" >&2
  printf '%s\n' "$duplicates" >&2
  exit 1
}
diff -u "$doctor_expected" "$doctor_documented"

# Keep the lock-drift table aligned with every production LockDriftCode name
# so new drift categories cannot silently disappear from recovery guidance.
lockfile_source="$root/src/lockfile.rs"
lockfile_variants="$test_root/lock-drift-variants"
lockfile_expected="$test_root/lock-drift-expected-codes"
lockfile_table="$test_root/lock-drift-table-codes"

awk '
  /^pub enum LockDriftCode/ { in_enum = 1; next }
  in_enum && /^}/ { exit }
  in_enum && /^[[:space:]]+[A-Z][A-Za-z0-9_]*,/ {
    line = $0
    sub(/^[[:space:]]+/, "", line)
    sub(/,.*/, "", line)
    print line
  }
' "$lockfile_source" \
  | sort -u > "$lockfile_variants"

awk '
  /^fn drift_code_name/ { in_mapping = 1; next }
  in_mapping && /^}/ { exit }
  in_mapping && /LockDriftCode::/ {
    line = $0
    sub(/.*LockDriftCode::/, "", line)
    split(line, fields, / => "/)
    variant = fields[1]
    code = fields[2]
    sub(/".*/, "", code)
    print variant " " code
  }
' "$lockfile_source" \
  | while IFS=' ' read -r variant code; do
      grep -Fx "$variant" "$lockfile_variants" >/dev/null && printf '%s\n' "$code"
    done \
  | sort -u > "$lockfile_expected"

awk '
  /^### Lock Drift$/ { in_table = 1; next }
  in_table && /^### / { exit }
  in_table && /^\| `/ {
    line = $0
    sub(/^\| `/, "", line)
    split(line, fields, /`/)
    print fields[1]
  }
' "$root/docs/troubleshooting.md" \
  | tr ', ' '\n' \
  | sed '/^$/d; s/^`//; s/`$//' \
  > "$lockfile_table"

duplicates="$(sort "$lockfile_table" | uniq -d)"
test -z "$duplicates" || {
  echo 'duplicate lock-drift troubleshooting rows:' >&2
  printf '%s\n' "$duplicates" >&2
  exit 1
}
sort "$lockfile_table" -o "$lockfile_table"
diff -u "$lockfile_expected" "$lockfile_table"

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
grep -q 'dalo --store .* approve skill public:review-helper' "$test_root/status"
"$dalo" --store "$store" approve skill public:review-helper
"$dalo" --store "$store" sync
test -L "$target/review-helper"
"$dalo" source refresh --help | grep -q 'Exit non-zero when selected skills drifted upstream'

# The team guide's authoring flow: initialize a manifest, pin an external
# catalog to an exact commit, preview the next pin, then advance it.
team_repo="$test_root/team-repo"
upstream="$test_root/upstream-catalog"
mkdir -p "$upstream/skills/copywriting"
printf '# Copywriting\n' > "$upstream/skills/copywriting/SKILL.md"
git -C "$upstream" init -q -b main
git -C "$upstream" add .
git -C "$upstream" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -qm initial
upstream_first="$(git -C "$upstream" rev-parse HEAD)"
printf '# Copywriting\n\nLead with the customer outcome.\n' > "$upstream/skills/copywriting/SKILL.md"
git -C "$upstream" add .
git -C "$upstream" -c commit.gpgSign=false -c user.email=test@example.com -c user.name='Test User' commit -qm update
upstream_second="$(git -C "$upstream" rev-parse HEAD)"
mkdir -p "$team_repo"
git -C "$team_repo" init -q -b main
(
  cd "$team_repo"
  "$dalo" team init company --name 'Company Skills'
  "$dalo" team catalog add marketing "$upstream" --version "$upstream_first" --skill +copywriting
  "$dalo" team show | grep -Fq "version=$upstream_first"
  "$dalo" --dry-run team catalog update marketing --from main | grep -Fq 'would update'
  grep -Fq "version = \"$upstream_first\"" dalo.toml
  "$dalo" team catalog update marketing --from main | grep -Fq 'result: updated'
  grep -Fq "version = \"$upstream_second\"" dalo.toml
) > /dev/null

echo "documentation checks passed"

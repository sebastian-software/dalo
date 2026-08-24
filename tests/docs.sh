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
grep -q 'dalo audit sebastian:pr-review --reviewer auto' "$root/README.md"
grep -q 'Watch the 15-second demo' "$root/README.md"
! grep -q '20-second demo' "$root/README.md"
grep -q '15-second secure-sync demo' "$root/site/index.html"
! grep -R -q 'github-pr-auto-review' "$root/README.md" "$root/site"
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
grep -q 'security preflight: deterministic checks and compatible cached findings only; sync did not run an agent reviewer; passing is not a safety guarantee' "$root/video/src/QuickstartVideo.tsx"
grep -q 'durationInFrames={450}' "$root/video/src/Root.tsx"
! grep -R -q -E 'cdn\.jsdelivr\.net|AsciinemaPlayer|asciinema-player' "$root/site"
grep -q 'DALO_VERSION' "$root/site/install.md"
grep -q 'dalo-v<version>.*, `v<version>`.*, or `<version>`' "$root/site/install.md"
grep -q '`<version>`, `v<version>`, or `dalo-v<version>`' "$root/npm/README.md"
! grep -q -E 'dalo-v0\.6\.1|v0\.7\.0|dalo-v0\.7\.0' "$root/site/install.md" "$root/npm/README.md"
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
  /^### `dalo agent list\|show <source>:<name>`$/ { in_section = 1; next }
  in_section && /^### / { exit }
  in_section { print }
' "$root/docs/reference.md")"
printf '%s\n' "$reference_status_section" | grep -Fq '`--check` exits with code 1'
printf '%s\n' "$reference_status_section" | grep -Fq 'full report on stdout for JSON'
if printf '%s\n' "$reference_agent_section" | grep -Fq '`--check` exits with code 1'; then
  echo 'status --check semantics must not be documented under dalo agent' >&2
  exit 1
fi

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
}
assert_target_reference "$target_section" "$reference_document"

# A missing cross-reference or a non-published triplet must fail this gate.
missing_target_document="$(printf '%s\n' "$reference_document" | sed '/site\/install.md/d')"
if assert_target_reference "$target_section" "$missing_target_document"; then
  echo 'DALO_TARGET reference gate accepted a missing installer link' >&2
  exit 1
fi
wrong_target_section="$(printf '%s\n' "$target_section" | sed 's/aarch64-unknown-linux-musl/aarch64-unknown-linux-invalid/')"
if assert_target_reference "$wrong_target_section" "$reference_document"; then
  echo 'DALO_TARGET reference gate accepted a wrong published target' >&2
  exit 1
fi

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

echo "documentation checks passed"

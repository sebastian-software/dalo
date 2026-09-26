#!/bin/sh
# Exercise the documented CLI path in a clean, throwaway user environment.
#
# This is the local/CI part of the 1.0 release rehearsal. Distribution channels
# still need the manual smoke commands in docs/ci.md after the public tag exists.
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/dalo-release-rehearsal.XXXXXX")"
log_file="$test_root/rehearsal.log"

cleanup() {
  status=$?
  if [ "$status" -ne 0 ] && [ -f "$log_file" ]; then
    echo "release rehearsal failed; transcript:" >&2
    cat "$log_file" >&2
  fi
  rm -rf "$test_root"
  exit "$status"
}
trap cleanup EXIT INT TERM

binary="${DALO_REHEARSAL_BINARY:-}"
case "$binary" in
  "")
    cargo build --release --locked
    binary="$root/target/release/dalo"
    ;;
  /*) ;;
  *) binary="$root/$binary" ;;
esac
test -x "$binary"

rehearsal_home="$test_root/home"
store="$test_root/store"
target="$test_root/skills"
mkdir -p "$rehearsal_home" "$target"

export HOME="$rehearsal_home"
export DALO_STORE="$store"
export CODEX_HOME="$rehearsal_home/.codex"
export CLAUDE_CONFIG_DIR="$rehearsal_home/.claude"
export OPENCODE_CONFIG_DIR="$rehearsal_home/.config/opencode"
export XDG_CONFIG_HOME="$rehearsal_home/.config"
export XDG_DATA_HOME="$rehearsal_home/.local/share"
export XDG_CACHE_HOME="$rehearsal_home/.cache"
export DALO_UPDATE_CHECK=never
export DALO_ASSISTANT_CHECK=never
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_NOSYSTEM=1
unset GIT_CONFIG_COUNT GIT_CONFIG_PARAMETERS

run() {
  {
    printf '\n$ dalo'
    for argument in "$@"; do
      printf ' %s' "$argument"
    done
    printf '\n'
  } >> "$log_file"
  "$binary" "$@" >> "$log_file" 2>&1
}

check_assistant_state() {
  "$binary" assistant status --json > "$test_root/assistant-status.json"
  printf '\n$ dalo assistant status --json\n' >> "$log_file"
  cat "$test_root/assistant-status.json" >> "$log_file"
  node -e 'const fs = require("node:fs"); const assert = require("node:assert/strict");
    assert.equal(JSON.parse(fs.readFileSync(process.argv[1], "utf8")).state, process.argv[2]);' \
    "$test_root/assistant-status.json" "$1"
}

version_output="$("$binary" --version)"
printf '$ dalo --version\n%s\n' "$version_output" >> "$log_file"
if [ -n "${DALO_REHEARSAL_EXPECTED_VERSION:-}" ]; then
  test "$version_output" = "dalo ${DALO_REHEARSAL_EXPECTED_VERSION}"
fi

check_assistant_state missing_store
run init
check_assistant_state missing
run target detect
run target link generic "$target"
run assistant install --json
check_assistant_state current
test -f "$store/local/skills/dalo/SKILL.md"
test -f "$store/local/skills/dalo/agents/openai.yaml"
test -f "$store/local/skills/dalo/references/migration.md"
cp "$store/local/skills/dalo/.dalo-bundle.toml" "$test_root/assistant-receipt.toml"
run assistant install --json
cmp "$test_root/assistant-receipt.toml" "$store/local/skills/dalo/.dalo-bundle.toml"

mkdir -p "$store/local/skills/release-rehearsal"
cat > "$store/local/skills/release-rehearsal/SKILL.md" <<'EOF'
# Release rehearsal

Exercise the documented local skill path without touching a user's real agent folder.
EOF

run status --json
run sync
test -L "$target/release-rehearsal"
test -L "$target/dalo"
cmp "$store/local/skills/dalo/SKILL.md" "$target/dalo/SKILL.md"
run status --check --json
run doctor --check --json
run plan --json
run next

# A user-edited bundle is reported and never replaced by an update/install.
printf '\nMy custom rehearsal note.\n' >> "$store/local/skills/dalo/SKILL.md"
cp "$store/local/skills/dalo/SKILL.md" "$test_root/custom-skill.md"
check_assistant_state blocked
if run assistant install --json; then
  echo "assistant install unexpectedly replaced a user-edited bundle" >&2
  exit 1
fi
cmp "$test_root/custom-skill.md" "$store/local/skills/dalo/SKILL.md"
cmp "$test_root/custom-skill.md" "$target/dalo/SKILL.md"

cat "$log_file"
echo "release rehearsal passed"

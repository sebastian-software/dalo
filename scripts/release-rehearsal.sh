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

home="$test_root/home"
store="$test_root/store"
target="$test_root/skills"
mkdir -p "$home" "$target"

export HOME="$home"
export DALO_STORE="$store"

run() {
  printf '\n$ dalo'
  for argument in "$@"; do
    printf ' %s' "$argument"
  done
  printf '\n'
  "$binary" "$@" >> "$log_file" 2>&1
}

version_output="$($binary --version)"
printf '$ dalo --version\n%s\n' "$version_output" >> "$log_file"
if [ -n "${DALO_REHEARSAL_EXPECTED_VERSION:-}" ]; then
  test "$version_output" = "dalo ${DALO_REHEARSAL_EXPECTED_VERSION}"
fi

run init
run target detect
run target link generic "$target"

mkdir -p "$store/local/skills/release-rehearsal"
cat > "$store/local/skills/release-rehearsal/SKILL.md" <<'EOF'
# Release rehearsal

Exercise the documented local skill path without touching a user's real agent folder.
EOF

run status --json
run sync
test -L "$target/release-rehearsal"
run status --check --json
run doctor --check --json
run plan --json
run next

echo "release rehearsal passed"

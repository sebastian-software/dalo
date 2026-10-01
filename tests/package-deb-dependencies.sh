#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/dalo-deb-dependencies.XXXXXX")
cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT HUP INT TERM

mkdir -p "$test_root/bin" "$test_root/source/completions" "$test_root/source/man/man1"
cat > "$test_root/bin/readelf" <<'EOF'
#!/bin/sh
set -eu
case "$1" in
  -h)
    printf '  Machine: %s\n' "$DALO_TEST_MACHINE"
    ;;
  -d)
    for library in $DALO_TEST_NEEDED; do
      printf ' 0x0000000000000001 (NEEDED)             Shared library: [%s]\n' "$library"
    done
    ;;
  --version-info)
    printf '  Name: GLIBC_2.23\n'
    ;;
  *)
    echo "unexpected readelf arguments: $*" >&2
    exit 2
    ;;
esac
EOF
cat > "$test_root/bin/dpkg-deb" <<'EOF'
#!/bin/sh
set -eu
test "$1" = --root-owner-group
test "$2" = --build
cp "$3/DEBIAN/control" "$DALO_TEST_CONTROL"
: > "$4"
EOF
chmod +x "$test_root/bin/readelf" "$test_root/bin/dpkg-deb"
for file in dalo README.md LICENSE-MIT LICENSE-APACHE completions/dalo.bash completions/_dalo completions/dalo.fish man/man1/dalo.1; do
  mkdir -p "$test_root/source/$(dirname -- "$file")"
  : > "$test_root/source/$file"
done

run_package() {
  target=$1
  machine=$2
  needed=$3
  expected=$4
  export DALO_TEST_MACHINE=$machine DALO_TEST_NEEDED=$needed
  DALO_TEST_CONTROL="$test_root/control" PATH="$test_root/bin:$PATH" \
    sh "$root/scripts/package-deb.sh" "$test_root/source" "$version" "$target" "$test_root/dist" \
    > "$test_root/stdout" 2> "$test_root/stderr"
  grep -Fq "Architecture: $expected" "$test_root/control"
  grep -Fq 'Depends: git, libc6 (>= 2.23)' "$test_root/control"
}

reject_package() {
  target=$1
  machine=$2
  needed=$3
  if DALO_TEST_MACHINE=$machine DALO_TEST_NEEDED=$needed DALO_TEST_CONTROL="$test_root/control" \
    PATH="$test_root/bin:$PATH" \
    sh "$root/scripts/package-deb.sh" "$test_root/source" "$version" "$target" "$test_root/dist" \
    > "$test_root/stdout" 2> "$test_root/stderr"; then
    echo "package-deb.sh unexpectedly accepted dependencies: $needed" >&2
    exit 1
  fi
  grep -Fq 'unsupported or unaccounted binary dependency:' "$test_root/stderr"
}

run_package x86_64-unknown-linux-gnu 'Advanced Micro Devices X86-64' \
  'libc.so.6 ld-linux-x86-64.so.2' amd64
run_package aarch64-unknown-linux-gnu AArch64 \
  'libc.so.6 ld-linux-aarch64.so.1' arm64
reject_package x86_64-unknown-linux-gnu 'Advanced Micro Devices X86-64' \
  'libc.so.6 ld-linux-aarch64.so.1'
reject_package aarch64-unknown-linux-gnu AArch64 \
  'libc.so.6 libunaccounted.so.1'

echo 'Debian package dependency checks passed'

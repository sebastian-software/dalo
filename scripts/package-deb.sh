#!/bin/sh
set -eu
umask 022

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <release-package-dir> <version> <target> <output-dir>" >&2
  exit 2
fi

source_dir=$1
version=$2
target=$3
output_dir=$4
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

case "$version" in
  ''|*[!0-9A-Za-z.+:~_-]*)
    echo "invalid Debian package version: $version" >&2
    exit 1
    ;;
esac

source_version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
if [ "$version" != "$source_version" ]; then
  echo "release version $version does not match Cargo.toml version $source_version" >&2
  exit 1
fi

case "$target" in
  x86_64-unknown-linux-gnu) architecture=amd64 ;;
  aarch64-unknown-linux-gnu) architecture=arm64 ;;
  *)
    echo "unsupported Debian target: $target" >&2
    exit 1
    ;;
esac

for file in dalo README.md LICENSE-MIT LICENSE-APACHE completions/dalo.bash completions/_dalo completions/dalo.fish man/man1/dalo.1; do
  if [ ! -f "$source_dir/$file" ]; then
    echo "release package is missing $file" >&2
    exit 1
  fi
done

command -v dpkg-deb >/dev/null 2>&1 || {
  echo "dpkg-deb is required to build Debian packages" >&2
  exit 1
}
command -v readelf >/dev/null 2>&1 || {
  echo "readelf is required to validate Debian package binaries" >&2
  exit 1
}

case "$target" in
  x86_64-unknown-linux-gnu) expected_machine='Advanced Micro Devices X86-64' ;;
  aarch64-unknown-linux-gnu) expected_machine='AArch64' ;;
esac
machine=$(readelf -h "$source_dir/dalo" | sed -n 's/^[[:space:]]*Machine:[[:space:]]*//p')
if [ "$machine" != "$expected_machine" ]; then
  echo "binary machine '$machine' does not match target $target" >&2
  exit 1
fi

needed=$(readelf -d "$source_dir/dalo" | sed -n 's/.*Shared library: \[\(.*\)\].*/\1/p' | sort -u)
for library in $needed; do
  case "$target:$library" in
    # The ELF interpreter is installed by libc6; only accept the loader
    # matching the package architecture.
    x86_64-unknown-linux-gnu:ld-linux-x86-64.so.2|aarch64-unknown-linux-gnu:ld-linux-aarch64.so.1) ;;
    *:libc.so.6|*:libm.so.6|*:libdl.so.2|*:libpthread.so.0|*:librt.so.1|*:libgcc_s.so.1) ;;
    *)
      echo "unsupported or unaccounted binary dependency: $library" >&2
      exit 1
      ;;
  esac
done
printf '%s\n' "$needed" | grep -Fxq 'libc.so.6' || {
  echo "binary does not dynamically link libc.so.6" >&2
  exit 1
}

if readelf --version-info "$source_dir/dalo" | grep -Fq GLIBC_PRIVATE; then
  echo "binary depends on private glibc symbols" >&2
  exit 1
fi
glibc_required=$(readelf --version-info "$source_dir/dalo" \
  | sed -n 's/.*Name: GLIBC_\([0-9.]*\).*/\1/p' \
  | sort -V \
  | tail -n 1)
if [ -z "$glibc_required" ]; then
  echo "could not determine the binary's minimum glibc symbol version" >&2
  exit 1
fi
if [ -n "${DALO_MAX_GLIBC_VERSION:-}" ] \
  && [ "$(printf '%s\n' "$DALO_MAX_GLIBC_VERSION" "$glibc_required" | sort -V | tail -n 1)" != "$DALO_MAX_GLIBC_VERSION" ]; then
  echo "binary requires glibc $glibc_required, above the release limit $DALO_MAX_GLIBC_VERSION" >&2
  exit 1
fi

# Published release binaries are built with cross 0.2.5 GNU images, whose
# documented minimum glibc is 2.23. Local CI binaries may need a newer version.
glibc_minimum=2.23
if [ "$(printf '%s\n' "$glibc_minimum" "$glibc_required" | sort -V | tail -n 1)" = "$glibc_required" ]; then
  glibc_minimum=$glibc_required
fi
depends="git, libc6 (>= $glibc_minimum)"
if printf '%s\n' "$needed" | grep -Fxq 'libgcc_s.so.1'; then
  depends="$depends, libgcc-s1 | libgcc1"
fi

temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/dalo-deb.XXXXXX")
cleanup() {
  rm -rf "$temporary_root"
}
trap cleanup EXIT HUP INT TERM

package_root="$temporary_root/package"
mkdir -p \
  "$package_root/DEBIAN" \
  "$package_root/usr/bin" \
  "$package_root/usr/share/bash-completion/completions" \
  "$package_root/usr/share/zsh/vendor-completions" \
  "$package_root/usr/share/fish/vendor_completions.d" \
  "$package_root/usr/share/man/man1" \
  "$package_root/usr/share/dalo" \
  "$package_root/usr/share/doc/dalo"

install -m 0755 "$source_dir/dalo" "$package_root/usr/bin/dalo"
install -m 0644 "$source_dir/completions/dalo.bash" "$package_root/usr/share/bash-completion/completions/dalo"
install -m 0644 "$source_dir/completions/_dalo" "$package_root/usr/share/zsh/vendor-completions/_dalo"
install -m 0644 "$source_dir/completions/dalo.fish" "$package_root/usr/share/fish/vendor_completions.d/dalo.fish"
install -m 0644 "$source_dir/man/man1/dalo.1" "$package_root/usr/share/man/man1/dalo.1"
printf '%s\n' debian > "$package_root/usr/share/dalo/.dalo-install-channel"
install -m 0644 "$source_dir/README.md" "$package_root/usr/share/doc/dalo/README.md"
install -m 0644 "$source_dir/LICENSE-MIT" "$package_root/usr/share/doc/dalo/LICENSE-MIT"
install -m 0644 "$source_dir/LICENSE-APACHE" "$package_root/usr/share/doc/dalo/LICENSE-APACHE"
cat > "$package_root/usr/share/doc/dalo/copyright" <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: dalo
Source: https://github.com/sebastian-software/dalo

Files: *
Copyright: 2026 Sebastian Software GmbH
License: MIT or Apache-2.0
 The complete license texts are available in /usr/share/doc/dalo/LICENSE-MIT
 and /usr/share/doc/dalo/LICENSE-APACHE.
EOF

cat > "$package_root/DEBIAN/control" <<EOF
Package: dalo
Version: $version
Section: utils
Priority: optional
Architecture: $architecture
Maintainer: Sebastian Werner <s.werner@sebastian-software.de>
Depends: $depends
Homepage: https://dalo.sh
Description: AI agent skill manager
 Dalo versions, reviews, and syncs AI agent skills through Git.
EOF

mkdir -p "$output_dir"
output="$output_dir/dalo_${version}_${architecture}.deb"
dpkg-deb --root-owner-group --build "$package_root" "$output"
(
  cd "$output_dir"
  shasum -a 256 "$(basename -- "$output")" > "$(basename -- "$output").sha256"
)
printf 'Built %s\n' "$output"

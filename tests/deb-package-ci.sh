#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <release-binary> <rust-target>" >&2
  exit 2
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
sh "$root/tests/package-deb-dependencies.sh"
binary=$1
target=$2
version=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
. /etc/os-release
if [ "${ID:-}" != ubuntu ] || [ -z "${VERSION_ID:-}" ]; then
  echo "Debian package lifecycle test requires an Ubuntu CI runner" >&2
  exit 1
fi
# Docker's official ECR mirror avoids Docker Hub's anonymous pull limits.
ubuntu_image="public.ecr.aws/docker/library/ubuntu:${VERSION_ID}"
test_root=$(mktemp -d "${TMPDIR:-/tmp}/dalo-deb-ci.XXXXXX")
cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT HUP INT TERM

package_dir="$test_root/dalo-${version}-${target}"
mkdir -p "$package_dir/completions" "$package_dir/man/man1"
cp "$binary" "$package_dir/dalo"
cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$package_dir/"
cargo run --locked --manifest-path "$root/Cargo.toml" -- completions bash > "$package_dir/completions/dalo.bash"
cargo run --locked --manifest-path "$root/Cargo.toml" -- completions zsh > "$package_dir/completions/_dalo"
cargo run --locked --manifest-path "$root/Cargo.toml" -- completions fish > "$package_dir/completions/dalo.fish"
cargo run --locked --manifest-path "$root/Cargo.toml" -- manpage > "$package_dir/man/man1/dalo.1"

sh "$root/scripts/package-deb.sh" "$package_dir" "$version" "$target" "$test_root/dist"
case "$target" in
  x86_64-unknown-linux-gnu) architecture=amd64 ;;
  aarch64-unknown-linux-gnu) architecture=arm64 ;;
  *) echo "unsupported Debian lifecycle target: $target" >&2; exit 2 ;;
esac
sh "$root/tests/deb-package-lifecycle.sh" "$test_root/dist/dalo_${version}_${architecture}.deb" "$version" "$architecture" "$ubuntu_image"

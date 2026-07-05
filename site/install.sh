#!/bin/sh
set -eu

repo="sebastian-software/dalo"
base_url="https://github.com/${repo}"
install_dir="${DALO_INSTALL_DIR:-$HOME/.local/bin}"
tmp_dir="${TMPDIR:-/tmp}/dalo-install.$$"

cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "dalo installer: missing required command: $1" >&2
    exit 1
  fi
}

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os:$arch" in
    Darwin:x86_64) echo "x86_64-apple-darwin" ;;
    Darwin:arm64) echo "aarch64-apple-darwin" ;;
    Linux:x86_64) echo "${DALO_LINUX_LIBC:-x86_64-unknown-linux-gnu}" ;;
    Linux:aarch64 | Linux:arm64) echo "${DALO_LINUX_LIBC:-aarch64-unknown-linux-gnu}" ;;
    *)
      echo "dalo installer: unsupported platform: $os $arch" >&2
      echo "Supported targets: x86_64/aarch64 Linux and macOS." >&2
      exit 1
      ;;
  esac
}

sha_check() {
  checksum_file="$1"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c "$checksum_file"
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c "$checksum_file"
  else
    echo "dalo installer: missing shasum or sha256sum" >&2
    exit 1
  fi
}

latest_tag() {
  curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' |
    head -n 1
}

need curl
need tar
target="${DALO_TARGET:-$(detect_target)}"
tag="${DALO_VERSION:-$(latest_tag)}"

if [ -z "$tag" ]; then
  echo "dalo installer: could not resolve latest release tag" >&2
  exit 1
fi

case "$tag" in
  dalo-v*) version="${tag#dalo-v}" ;;
  v*) version="${tag#v}" ;;
  *) version="$tag" ;;
esac

package="dalo-${version}-${target}"
archive="${package}.tar.gz"
mkdir -p "$tmp_dir" "$install_dir"

echo "Installing dalo ${version} for ${target}"
curl -fL "${base_url}/releases/download/${tag}/${archive}" -o "${tmp_dir}/${archive}"
curl -fL "${base_url}/releases/download/${tag}/${archive}.sha256" -o "${tmp_dir}/${archive}.sha256"

(
  cd "$tmp_dir"
  sha_check "${archive}.sha256"
  tar xzf "$archive"
)

install -m 0755 "${tmp_dir}/${package}/dalo" "${install_dir}/dalo"

if [ -d "${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions" ]; then
  install -m 0644 "${tmp_dir}/${package}/completions/dalo.bash" \
    "${XDG_DATA_HOME:-$HOME/.local/share}/bash-completion/completions/dalo"
fi
if [ -d "${ZDOTDIR:-$HOME}/.zfunc" ]; then
  install -m 0644 "${tmp_dir}/${package}/completions/_dalo" "${ZDOTDIR:-$HOME}/.zfunc/_dalo"
fi
if [ -d "${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions" ]; then
  install -m 0644 "${tmp_dir}/${package}/completions/dalo.fish" \
    "${XDG_CONFIG_HOME:-$HOME/.config}/fish/completions/dalo.fish"
fi
if [ -d "${XDG_DATA_HOME:-$HOME/.local/share}/man/man1" ]; then
  install -m 0644 "${tmp_dir}/${package}/man/man1/dalo.1" \
    "${XDG_DATA_HOME:-$HOME/.local/share}/man/man1/dalo.1"
fi

echo "Installed: ${install_dir}/dalo"
echo "Verify: ${install_dir}/dalo --version"

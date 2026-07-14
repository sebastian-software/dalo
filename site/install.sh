#!/bin/sh
set -eu

repo="sebastian-software/dalo"
base_url="https://github.com/${repo}"
install_dir="${DALO_INSTALL_DIR:-$HOME/.local/bin}"
verify_mode="${DALO_VERIFY:-auto}"
umask 077
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/dalo-install.XXXXXX")"

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
    Linux:x86_64) echo "x86_64-unknown-linux-$(detect_linux_libc)" ;;
    Linux:aarch64 | Linux:arm64) echo "aarch64-unknown-linux-$(detect_linux_libc)" ;;
    *)
      echo "dalo installer: unsupported platform: $os $arch" >&2
      echo "Supported targets: x86_64/aarch64 Linux and macOS." >&2
      exit 1
      ;;
  esac
}

detect_linux_libc() {
  libc="${DALO_LINUX_LIBC:-}"
  if [ -n "$libc" ]; then
    case "$libc" in
      gnu | musl) echo "$libc"; return ;;
      *) echo "dalo installer: invalid DALO_LINUX_LIBC value: $libc (use gnu or musl)" >&2; exit 1 ;;
    esac
  fi
  if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
    echo musl
  elif ls /lib/ld-musl-*.so.1 >/dev/null 2>&1; then
    echo musl
  else
    echo gnu
  fi
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
  tag="$(curl -fsSL "https://api.github.com/repos/${repo}/releases/latest" 2>/dev/null |
    sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' |
    head -n 1 || true)"
  if [ -n "$tag" ]; then
    printf '%s\n' "$tag"
    return
  fi

  redirect_url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "${base_url}/releases/latest" 2>/dev/null || true)"
  case "$redirect_url" in
    */releases/tag/*) printf '%s\n' "${redirect_url##*/}" ;;
  esac
}

case "$verify_mode" in
  auto | required) ;;
  *)
    echo "dalo installer: invalid DALO_VERIFY value: $verify_mode" >&2
    echo "Supported values: auto, required." >&2
    exit 1
    ;;
esac

need curl
need tar
if [ "$verify_mode" = "required" ] && ! command -v cosign >/dev/null 2>&1; then
  echo "dalo installer: cosign is required when DALO_VERIFY=required" >&2
  echo "Install cosign from https://docs.sigstore.dev/cosign/system_config/installation/ and retry." >&2
  exit 1
fi
target="${DALO_TARGET:-$(detect_target)}"
tag="${DALO_VERSION:-$(latest_tag)}"

if [ -z "$tag" ]; then
  echo "dalo installer: could not resolve latest release tag" >&2
  exit 1
fi

case "$tag" in
  dalo-v*) ;;
  v*) tag="dalo-${tag}" ;;
  *) tag="dalo-v${tag}" ;;
esac
version="${tag#dalo-v}"

package="dalo-${version}-${target}"
archive="${package}.tar.gz"
mkdir -p "$install_dir"

echo "Installing dalo ${version} for ${target}"
curl -fL "${base_url}/releases/download/${tag}/${archive}" -o "${tmp_dir}/${archive}"
curl -fL "${base_url}/releases/download/${tag}/${archive}.sha256" -o "${tmp_dir}/${archive}.sha256"

verify_bundle=0
if command -v cosign >/dev/null 2>&1; then
  if curl -fL "${base_url}/releases/download/${tag}/${archive}.sigstore.json" \
    -o "${tmp_dir}/${archive}.sigstore.json"; then
    verify_bundle=1
  elif [ "$verify_mode" = "required" ]; then
    echo "dalo installer: no Sigstore bundle for ${tag}; DALO_VERIFY=required cannot continue" >&2
    exit 1
  else
    echo "dalo installer: no Sigstore bundle for ${tag}; falling back to checksum-only verification" >&2
  fi
else
  echo "dalo installer: cosign not found; verifying the SHA-256 checksum only" >&2
  echo "dalo installer: use DALO_VERIFY=required to require Sigstore provenance verification" >&2
fi

(
  cd "$tmp_dir"
  sha_check "${archive}.sha256"
  if [ "$verify_bundle" -eq 1 ]; then
    cosign verify-blob "$archive" \
      --bundle "${archive}.sigstore.json" \
      --certificate-identity-regexp '^https://github\.com/sebastian-software/dalo/\.github/workflows/(release-please|publish)\.yml@refs/heads/main$' \
      --certificate-oidc-issuer "https://token.actions.githubusercontent.com"
  fi
  tar xzf "$archive"
)

install -m 0755 "${tmp_dir}/${package}/dalo" "${install_dir}/dalo"
printf '%s\n' standalone > "${install_dir}/.dalo-install-channel"

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

case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *)
    echo "dalo installer: ${install_dir} is not on PATH" >&2
    echo "Add this for the current shell: export PATH=\"${install_dir}:\$PATH\"" >&2
    ;;
esac

existing_dalo="$(command -v dalo 2>/dev/null || true)"
if [ -n "$existing_dalo" ] && [ "$existing_dalo" != "${install_dir}/dalo" ]; then
  echo "dalo installer: ${existing_dalo} shadows the newly installed ${install_dir}/dalo" >&2
fi

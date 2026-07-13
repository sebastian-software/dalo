#!/bin/sh
set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/dalo-install-test.XXXXXX")"

cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT INT TERM

package="dalo-9.8.7-x86_64-unknown-linux-gnu"
fixture_dir="${test_root}/fixture"
mkdir -p "${fixture_dir}/${package}/completions" "${fixture_dir}/${package}/man/man1"
printf '#!/bin/sh\necho dalo 9.8.7\n' > "${fixture_dir}/${package}/dalo"
printf 'bash completion\n' > "${fixture_dir}/${package}/completions/dalo.bash"
printf 'zsh completion\n' > "${fixture_dir}/${package}/completions/_dalo"
printf 'fish completion\n' > "${fixture_dir}/${package}/completions/dalo.fish"
printf 'man page\n' > "${fixture_dir}/${package}/man/man1/dalo.1"
tar -C "$fixture_dir" -czf "${fixture_dir}/${package}.tar.gz" "$package"
(
  cd "$fixture_dir"
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${package}.tar.gz" > "${package}.tar.gz.sha256"
  else
    sha256sum "${package}.tar.gz" > "${package}.tar.gz.sha256"
  fi
)
printf '{}\n' > "${fixture_dir}/${package}.tar.gz.sigstore.json"

make_path() {
  path_dir="$1"
  mkdir -p "$path_dir"
  for command_name in cp gzip head install mkdir mktemp rm sed shasum sha256sum tar uname; do
    command_path="$(command -v "$command_name" 2>/dev/null || true)"
    if [ -n "$command_path" ]; then
      ln -s "$command_path" "${path_dir}/${command_name}"
    fi
  done
  cp "${repo_root}/tests/support/fake-installer-curl.sh" "${path_dir}/curl"
  chmod +x "${path_dir}/curl"
}

run_install() {
  path_dir="$1"
  install_dir="$2"
  output_file="$3"
  shift 3
  env \
    PATH="$path_dir" \
    HOME="${test_root}/home" \
    DALO_INSTALL_DIR="$install_dir" \
    DALO_TARGET="x86_64-unknown-linux-gnu" \
    DALO_VERSION="dalo-v9.8.7" \
    DALO_INSTALLER_FIXTURES="$fixture_dir" \
    "$@" \
    /bin/sh "${repo_root}/site/install.sh" > "$output_file" 2>&1
}

auto_path="${test_root}/auto-path"
make_path "$auto_path"
auto_output="${test_root}/auto-output"
run_install "$auto_path" "${test_root}/auto-bin" "$auto_output"
test -x "${test_root}/auto-bin/dalo"
grep -q 'cosign not found; verifying the SHA-256 checksum only' "$auto_output"
grep -q 'is not on PATH' "$auto_output"

cosign_path="${test_root}/cosign-path"
make_path "$cosign_path"
cp "${repo_root}/tests/support/fake-cosign.sh" "${cosign_path}/cosign"
chmod +x "${cosign_path}/cosign"
cosign_output="${test_root}/cosign-output"
run_install "$cosign_path" "${test_root}/cosign-bin" "$cosign_output" \
  DALO_COSIGN_LOG="${test_root}/cosign.log"
test -x "${test_root}/cosign-bin/dalo"
grep -q -- '--certificate-identity' "${test_root}/cosign.log"
grep -q -- '--certificate-oidc-issuer' "${test_root}/cosign.log"

missing_bundle_output="${test_root}/missing-bundle-output"
run_install "$cosign_path" "${test_root}/missing-bundle-bin" "$missing_bundle_output" \
  DALO_FAKE_MISSING_BUNDLE=1
test -x "${test_root}/missing-bundle-bin/dalo"
grep -q 'no Sigstore bundle for dalo-v9.8.7; falling back to checksum-only verification' "$missing_bundle_output"

required_bundle_output="${test_root}/required-bundle-output"
if run_install "$cosign_path" "${test_root}/required-bundle-bin" "$required_bundle_output" \
  DALO_VERIFY=required DALO_FAKE_MISSING_BUNDLE=1; then
  echo "expected DALO_VERIFY=required to fail without a Sigstore bundle" >&2
  exit 1
fi
grep -q 'no Sigstore bundle for dalo-v9.8.7; DALO_VERIFY=required cannot continue' "$required_bundle_output"

required_output="${test_root}/required-output"
if run_install "$auto_path" "${test_root}/required-bin" "$required_output" DALO_VERIFY=required; then
  echo "expected DALO_VERIFY=required to fail without cosign" >&2
  exit 1
fi
grep -q 'cosign is required when DALO_VERIFY=required' "$required_output"
test ! -e "${test_root}/required-bin/dalo"

version_output="${test_root}/version-output"
run_install "$auto_path" "${test_root}/version-bin" "$version_output" DALO_VERSION=v9.8.7
test -x "${test_root}/version-bin/dalo"
grep -q 'Installing dalo 9.8.7' "$version_output"

plain_version_output="${test_root}/plain-version-output"
run_install "$auto_path" "${test_root}/plain-version-bin" "$plain_version_output" DALO_VERSION=9.8.7
test -x "${test_root}/plain-version-bin/dalo"

latest_fallback_output="${test_root}/latest-fallback-output"
run_install "$auto_path" "${test_root}/latest-fallback-bin" "$latest_fallback_output" \
  DALO_VERSION= DALO_FAKE_LATEST_API_FAIL=1
test -x "${test_root}/latest-fallback-bin/dalo"
grep -q 'Installing dalo 9.8.7' "$latest_fallback_output"

shadow_path="${test_root}/shadow-path"
make_path "$shadow_path"
printf '#!/bin/sh\necho stale dalo\n' > "${shadow_path}/dalo"
chmod +x "${shadow_path}/dalo"
shadow_output="${test_root}/shadow-output"
run_install "$shadow_path" "${test_root}/shadow-bin" "$shadow_output"
grep -q 'shadows the newly installed' "$shadow_output"

echo "installer tests passed"

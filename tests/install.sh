#!/bin/sh
set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/dalo-install-test.XXXXXX")"

cleanup() {
  rm -rf "$test_root"
}
trap cleanup EXIT INT TERM

fixture_dir="${test_root}/fixture"

make_fixture() {
  fixture_version="$1"
  fixture_package="dalo-${fixture_version}-x86_64-unknown-linux-gnu"
  mkdir -p "${fixture_dir}/${fixture_package}/completions" \
    "${fixture_dir}/${fixture_package}/man/man1"
  printf '#!/bin/sh\necho dalo %s\n' "$fixture_version" > "${fixture_dir}/${fixture_package}/dalo"
  printf 'bash completion\n' > "${fixture_dir}/${fixture_package}/completions/dalo.bash"
  printf 'zsh completion\n' > "${fixture_dir}/${fixture_package}/completions/_dalo"
  printf 'fish completion\n' > "${fixture_dir}/${fixture_package}/completions/dalo.fish"
  printf 'man page\n' > "${fixture_dir}/${fixture_package}/man/man1/dalo.1"
  tar -C "$fixture_dir" -czf "${fixture_dir}/${fixture_package}.tar.gz" "$fixture_package"
  (
    cd "$fixture_dir"
    if command -v shasum >/dev/null 2>&1; then
      shasum -a 256 "${fixture_package}.tar.gz" > "${fixture_package}.tar.gz.sha256"
    else
      sha256sum "${fixture_package}.tar.gz" > "${fixture_package}.tar.gz.sha256"
    fi
  )
  printf '{}\n' > "${fixture_dir}/${fixture_package}.tar.gz.sigstore.json"
}

package="dalo-9.8.7-x86_64-unknown-linux-gnu"
make_fixture 9.8.7
make_fixture 1.0.0

make_path() {
  path_dir="$1"
  mkdir -p "$path_dir"
  for command_name in cp gzip head install mkdir mktemp mv rm sed shasum sha256sum tar uname; do
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
curl_log="${test_root}/curl.log"
run_install "$auto_path" "${test_root}/auto-bin" "$auto_output" \
  DALO_FAKE_CURL_LOG="$curl_log"
test -x "${test_root}/auto-bin/dalo"
test "$(cat "${test_root}/auto-bin/.dalo-install-channel")" = standalone
grep -q 'cosign not found; verifying the SHA-256 checksum only' "$auto_output"
grep -q 'is not on PATH' "$auto_output"
grep -q -- '--connect-timeout 10' "$curl_log"
grep -q -- '--max-time 120' "$curl_log"
grep -q -- '--retry 2' "$curl_log"
grep -q -- "--proto =https" "$curl_log"
grep -q -- '--tlsv1.2' "$curl_log"

# The installer must stage downloads in a private directory it created itself.
# A predictable, pre-creatable path lets another local user plant a symlink that
# `curl -o` would then follow with the installing user's permissions.
sh -n "${repo_root}/site/install.sh"
if grep -q 'dalo-install\.\$\$' "${repo_root}/site/install.sh"; then
  echo "installer rebuilt a predictable PID-based temp path" >&2
  exit 1
fi

mktemp_path="${test_root}/mktemp-path"
make_path "$mktemp_path"
real_mktemp="$(command -v mktemp)"
rm -f "${mktemp_path}/mktemp"
mktemp_log="${test_root}/mktemp.log"
cat > "${mktemp_path}/mktemp" <<EOF
#!/bin/sh
printf 'umask=%s args=%s\n' "\$(umask)" "\$*" >> "${mktemp_log}"
exec "${real_mktemp}" "\$@"
EOF
chmod +x "${mktemp_path}/mktemp"
mktemp_tmpdir="${test_root}/mktemp-tmp"
mkdir -p "$mktemp_tmpdir"
mktemp_output="${test_root}/mktemp-output"
run_install "$mktemp_path" "${test_root}/mktemp-bin" "$mktemp_output" \
  TMPDIR="$mktemp_tmpdir"
test -x "${test_root}/mktemp-bin/dalo"
grep -q -- "-d ${mktemp_tmpdir}/dalo-install.XXXXXX" "$mktemp_log"
if ! grep -q '^umask=0\{0,1\}077 ' "$mktemp_log"; then
  echo "installer staged downloads without a private umask:" >&2
  cat "$mktemp_log" >&2
  exit 1
fi
set -- "${mktemp_tmpdir}"/dalo-install.*
test ! -e "$1"

# A private staging directory is a precondition, not a nicety: without one the
# installer must stop rather than fall back to a shared path.
refuse_path="${test_root}/refuse-path"
make_path "$refuse_path"
rm -f "${refuse_path}/mktemp"
printf '#!/bin/sh\nexit 1\n' > "${refuse_path}/mktemp"
chmod +x "${refuse_path}/mktemp"
refuse_output="${test_root}/refuse-output"
if run_install "$refuse_path" "${test_root}/refuse-bin" "$refuse_output"; then
  echo "expected the installer to fail without a private temp directory" >&2
  exit 1
fi
test ! -e "${test_root}/refuse-bin/dalo"

atomic_path="${test_root}/atomic-path"
make_path "$atomic_path"
rm -f "${atomic_path}/mv"
printf '#!/bin/sh\nexit 1\n' > "${atomic_path}/mv"
chmod +x "${atomic_path}/mv"
atomic_bin="${test_root}/atomic-bin"
mkdir -p "$atomic_bin"
printf 'previous binary\n' > "${atomic_bin}/dalo"
atomic_output="${test_root}/atomic-output"
if run_install "$atomic_path" "$atomic_bin" "$atomic_output"; then
  echo "expected an atomic install rename failure" >&2
  exit 1
fi
test "$(cat "${atomic_bin}/dalo")" = "previous binary"
set -- "${atomic_bin}"/.dalo.tmp.*
test ! -e "$1"

cosign_path="${test_root}/cosign-path"
make_path "$cosign_path"
cp "${repo_root}/tests/support/fake-cosign.sh" "${cosign_path}/cosign"
chmod +x "${cosign_path}/cosign"
cosign_output="${test_root}/cosign-output"
run_install "$cosign_path" "${test_root}/cosign-bin" "$cosign_output" \
  DALO_COSIGN_LOG="${test_root}/cosign.log"
test -x "${test_root}/cosign-bin/dalo"
grep -q -- '--certificate-identity-regexp' "${test_root}/cosign.log"
publish_workflow="${repo_root}/.github/workflows/publish.yml"
grep -Fq 'cosign sign-blob' "$publish_workflow"
publish_identity='^https://github\.com/sebastian-software/dalo/\.github/workflows/publish\.yml@refs/heads/main$'
grep -Fq -- "$publish_identity" "${test_root}/cosign.log"
if grep -Fq -- 'release-please\.yml@refs/heads/main' "${test_root}/cosign.log"; then
  echo "installer accepted a retired release-please workflow identity" >&2
  exit 1
fi
grep -q -- '--certificate-oidc-issuer' "${test_root}/cosign.log"

missing_bundle_output="${test_root}/missing-bundle-output"
if ! run_install "$cosign_path" "${test_root}/missing-bundle-bin" "$missing_bundle_output" \
  DALO_FAKE_MISSING_BUNDLE=1; then
  echo "auto-mode missing-bundle install failed:" >&2
  sed -n '1,120p' "$missing_bundle_output" >&2
  exit 1
fi
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

# The 0.x to 1.0.0 step: every accepted DALO_VERSION spelling has to resolve to
# the same `dalo-v1.0.0` tag and `dalo-1.0.0-*` archive, with neither a doubled
# nor a missing `v` anywhere in the download URLs.
major_case=0
for requested_version in 1.0.0 v1.0.0 dalo-v1.0.0; do
  major_case=$((major_case + 1))
  major_output="${test_root}/major-${major_case}-output"
  major_log="${test_root}/major-${major_case}.log"
  run_install "$auto_path" "${test_root}/major-${major_case}-bin" "$major_output" \
    DALO_VERSION="$requested_version" DALO_FAKE_CURL_LOG="$major_log"
  test -x "${test_root}/major-${major_case}-bin/dalo"
  grep -q 'Installing dalo 1.0.0' "$major_output"
  grep -Fq 'releases/download/dalo-v1.0.0/dalo-1.0.0-x86_64-unknown-linux-gnu.tar.gz' "$major_log"
  if grep -Fq 'dalo-vv' "$major_log"; then
    echo "DALO_VERSION=${requested_version} produced a doubled version prefix" >&2
    exit 1
  fi
  if grep -Eq 'releases/download/(dalo-)?[0-9]' "$major_log"; then
    echo "DALO_VERSION=${requested_version} produced a tag without its v prefix" >&2
    exit 1
  fi
done

latest_fallback_output="${test_root}/latest-fallback-output"
run_install "$auto_path" "${test_root}/latest-fallback-bin" "$latest_fallback_output" \
  DALO_VERSION= DALO_FAKE_LATEST_API_FAIL=1
test -x "${test_root}/latest-fallback-bin/dalo"
grep -q 'Installing dalo 9.8.7' "$latest_fallback_output"

# An Intel Mac has to be told that the build was discontinued and where to go
# instead. Without this the installer would compose a URL for an archive that
# no release produces and fail on a bare curl 404.
intel_path="${test_root}/intel-path"
make_path "$intel_path"
rm -f "${intel_path}/uname"
cat > "${intel_path}/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo Darwin ;;
  -m) echo x86_64 ;;
  *) echo Darwin ;;
esac
EOF
chmod +x "${intel_path}/uname"
intel_output="${test_root}/intel-output"
intel_log="${test_root}/intel-curl.log"
if env \
  PATH="$intel_path" \
  HOME="${test_root}/home" \
  DALO_INSTALL_DIR="${test_root}/intel-bin" \
  DALO_TARGET= \
  DALO_VERSION="dalo-v9.8.7" \
  DALO_INSTALLER_FIXTURES="$fixture_dir" \
  DALO_FAKE_CURL_LOG="$intel_log" \
  /bin/sh "${repo_root}/site/install.sh" > "$intel_output" 2>&1; then
  echo "expected the installer to refuse an Intel Mac" >&2
  cat "$intel_output" >&2
  exit 1
fi
grep -q 'Intel Macs are no longer supported' "$intel_output"
grep -q 'discontinued with Dalo 1.0' "$intel_output"
grep -q 'cargo install dalo' "$intel_output"
test ! -e "${test_root}/intel-bin/dalo"
# It must refuse before reaching the network, not after a failed download.
if [ -e "$intel_log" ] && grep -Fq 'x86_64-apple-darwin' "$intel_log"; then
  echo "the installer tried to download an Intel macOS archive" >&2
  exit 1
fi

# An Apple Silicon Mac keeps resolving its own target through the same path.
silicon_path="${test_root}/silicon-path"
make_path "$silicon_path"
rm -f "${silicon_path}/uname"
cat > "${silicon_path}/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo Darwin ;;
  -m) echo arm64 ;;
  *) echo Darwin ;;
esac
EOF
chmod +x "${silicon_path}/uname"
silicon_output="${test_root}/silicon-output"
env \
  PATH="$silicon_path" \
  HOME="${test_root}/home" \
  DALO_INSTALL_DIR="${test_root}/silicon-bin" \
  DALO_VERSION="dalo-v9.8.7" \
  DALO_INSTALLER_FIXTURES="$fixture_dir" \
  /bin/sh "${repo_root}/site/install.sh" > "$silicon_output" 2>&1 || true
grep -q 'Installing dalo 9.8.7 for aarch64-apple-darwin' "$silicon_output"

shadow_path="${test_root}/shadow-path"
make_path "$shadow_path"
printf '#!/bin/sh\necho stale dalo\n' > "${shadow_path}/dalo"
chmod +x "${shadow_path}/dalo"
shadow_output="${test_root}/shadow-output"
run_install "$shadow_path" "${test_root}/shadow-bin" "$shadow_output"
grep -q 'shadows the newly installed' "$shadow_output"

echo "installer tests passed"

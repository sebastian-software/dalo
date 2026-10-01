#!/bin/sh
set -eu

if [ "$#" -ne 4 ]; then
  echo "usage: $0 <package.deb> <version> <architecture> <ubuntu-image>" >&2
  exit 2
fi

package=$(CDPATH= cd -- "$(dirname -- "$1")" && pwd)/$(basename -- "$1")
version=$2
architecture=$3
ubuntu_image=$4
command -v docker >/dev/null 2>&1 || {
  echo "Docker is required for the isolated Debian package lifecycle test" >&2
  exit 1
}

case "$(uname -m):$architecture" in
  x86_64:amd64) platform=linux/amd64 ;;
  aarch64:arm64|arm64:arm64) platform=linux/arm64 ;;
  *) echo "host architecture does not match Debian package architecture $architecture" >&2; exit 1 ;;
esac

docker run --rm -i --platform "$platform" \
  --mount "type=bind,src=$package,dst=/tmp/dalo.deb,readonly" \
  "$ubuntu_image" bash -euo pipefail -s -- "$version" <<'CONTAINER'
version=$1
test_root=$(mktemp -d /tmp/dalo-package-lifecycle.XXXXXX)
trap 'rm -rf "$test_root"' EXIT
on_failure() {
  status=$1
  line=$2
  command=$3
  trap - ERR
  echo "lifecycle assertion failed at line $line: $command" >&2
  echo 'dpkg path filters in the disposable Ubuntu image:' >&2
  grep -R -E '^[[:space:]]*path-(exclude|include)[[:space:]=]+' /etc/dpkg 2>/dev/null >&2 || true
  dpkg-query -L dalo 2>/dev/null >&2 || true
  exit "$status"
}
trap 'on_failure "$?" "$LINENO" "$BASH_COMMAND"' ERR
mkdir -p "$test_root/store/local/skills" "$test_root/agent/skills"
printf 'keep-store\n' > "$test_root/store/local/skills/marker"
printf 'keep-target\n' > "$test_root/agent/skills/marker"

apt-get update
dpkg-deb --contents /tmp/dalo.deb > "$test_root/package-contents"
grep -Fq './usr/share/man/man1/dalo.1' "$test_root/package-contents"
dpkg_options=(-o 'Dpkg::Options::=--path-include=/usr/share/man/man1/dalo.1')
if grep -R -Eq '^[[:space:]]*path-exclude.*(/usr/share/man|/usr/share/\*/man)' /etc/dpkg 2>/dev/null; then
  echo 'dpkg path-excludes manpages in this Ubuntu image; including Dalo manpage for lifecycle verification'
fi
dpkg-deb --control /tmp/dalo.deb "$test_root/control"
for script in preinst postinst prerm postrm; do
  test ! -e "$test_root/control/$script"
done
dpkg-deb --extract /tmp/dalo.deb "$test_root/oldroot"
mkdir -p "$test_root/oldroot/DEBIAN"
cp -a "$test_root/control/." "$test_root/oldroot/DEBIAN/"
sed -i 's/^Version: .*/Version: 0.0.0/' "$test_root/oldroot/DEBIAN/control"
dpkg-deb --build --root-owner-group "$test_root/oldroot" "$test_root/dalo-old.deb"

apt-get "${dpkg_options[@]}" install -y "$test_root/dalo-old.deb"
dalo --version | grep -F "$version"
test "$(dpkg-query -W dalo | awk '{print $2}')" = 0.0.0
test "$(cat /usr/share/dalo/.dalo-install-channel)" = debian
test -x /usr/bin/dalo
test -f /usr/share/man/man1/dalo.1
test -f /usr/share/bash-completion/completions/dalo
test -f /usr/share/zsh/vendor-completions/_dalo
test -f /usr/share/fish/vendor_completions.d/dalo.fish

apt-get "${dpkg_options[@]}" install -y /tmp/dalo.deb
test "$(dpkg-query -W dalo | awk '{print $2}')" = "$version"
apt-get remove -y dalo
test ! -e /usr/bin/dalo
test ! -e /usr/share/dalo/.dalo-install-channel
test "$(cat "$test_root/store/local/skills/marker")" = keep-store
test "$(cat "$test_root/agent/skills/marker")" = keep-target
echo "Debian package install, upgrade, removal, and state-preservation checks passed"
CONTAINER

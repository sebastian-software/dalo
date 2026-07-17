# Install Dalo With an Agent

You are helping a user install Dalo, a Git-backed skill manager for AI agents.

## Constraints

- Only install on macOS or Linux.
- Do not modify shell startup files automatically.
- Do not touch real agent skill folders until the user confirms which agent target to link.
- On macOS with Homebrew, prefer the official
  `sebastian-software/tap/dalo` formula.
- Otherwise, prefer the official installer at `https://dalo.sh/install.sh`.
- The installer always verifies the release checksum and additionally verifies
  Sigstore provenance when `cosign` is available.
- Verify the installed binary with `dalo --version` and `dalo doctor`.

## Steps

1. Detect the platform:

   ```sh
   uname -s
   uname -m
   ```

2. Install the latest release. On macOS with Homebrew:

   ```sh
   brew install sebastian-software/tap/dalo
   ```

   Otherwise, use the hosted installer:

   ```sh
   curl -fsSL https://dalo.sh/install.sh | sh
   ```

   For environments that require strict Sigstore provenance verification and
   have Homebrew available, install Cosign and make verification mandatory:

   ```sh
   brew install cosign
   curl -fsSL https://dalo.sh/install.sh | DALO_VERIFY=required sh
   ```

   Without Homebrew, follow the official
   [Cosign installation guide](https://docs.sigstore.dev/cosign/system_config/installation/).

   Or, when the user manages command-line tools with mise, install the GitHub
   release through its GitHub Releases backend:

   ```sh
   mise use -g github:sebastian-software/dalo
   ```

   When the user manages CLI tools through Node.js 20 or newer, the npm launcher
   is also supported. It verifies release checksums and caches the downloaded
   binary under `~/.cache/dalo`:

   ```sh
   npx getdalo --version
   # or: npm install --global getdalo
   ```

   If the user wants a custom location, set `DALO_INSTALL_DIR`:

   ```sh
   curl -fsSL https://dalo.sh/install.sh | DALO_INSTALL_DIR="$HOME/.local/bin" sh
   ```

   Installer environment variables:

   - `DALO_VERSION` selects a release. Use `dalo-v<version>`, `v<version>`, or `<version>`.
   - `DALO_INSTALL_DIR` changes the binary destination (default: `~/.local/bin`).
   - `DALO_VERIFY=required` requires Sigstore provenance verification; `auto` is the default.
   - `DALO_LINUX_LIBC=gnu|musl` and `DALO_TARGET` override platform detection when needed.

3. Ensure the install directory is on `PATH` for this session:

   ```sh
   export PATH="$HOME/.local/bin:$PATH"
   ```

4. Verify the binary:

   ```sh
   dalo --version
   dalo doctor
   ```

5. Initialize Dalo:

   ```sh
   dalo init
   ```

6. Detect available agent targets:

   ```sh
   dalo target detect
   ```

7. Ask the user which target to link. Use one of:

   ```sh
   dalo target link codex
   dalo target link claude
   dalo target link openclaw
   dalo target link hermes
   ```

   For a sandbox or unsupported agent, use:

   ```sh
   dalo target link generic /path/to/skills
   ```

8. Optionally try a real public catalog. This selects one skill from
   [Sebastian's skill catalog](https://github.com/sebastian-software/skills.sebastian-software.com),
   then asks for the narrowest explicit approval before it can be linked:

   ```sh
   dalo source add-catalog sebastian https://github.com/sebastian-software/skills.sebastian-software.com.git
   dalo source inspect sebastian
   dalo source select sebastian pr-review
   dalo approve skill sebastian:pr-review
   dalo sync
   ```

9. Run a final health check:

   ```sh
   dalo status
   dalo doctor
   ```

## Notes

- Dalo shells out to `git` for source operations, so `git` must be on `PATH`.
- Windows is not supported yet. Recommend WSL for Windows users.
- If the installer cannot resolve a release or checksum, stop and show the error instead of downloading from an unofficial mirror.
- If the install directory is not on `PATH`, the installer prints the exact export command for the current shell.
- To remove a cached npm binary, delete `~/.cache/dalo`; uninstall a global
  launcher with `npm uninstall --global getdalo`.

## Manual Release Archives

Use the archive matching the machine from the
[latest GitHub release](https://github.com/sebastian-software/dalo/releases/latest).
Set `VERSION` without the leading `v` and choose one of the published targets:
`x86_64-apple-darwin`, `aarch64-apple-darwin`,
`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, or `aarch64-unknown-linux-musl`.

```sh
VERSION=REPLACE_WITH_RELEASE_VERSION
TARGET=aarch64-apple-darwin
PACKAGE="dalo-${VERSION}-${TARGET}"
ARCHIVE="${PACKAGE}.tar.gz"
BASE_URL="https://github.com/sebastian-software/dalo/releases/download/dalo-v${VERSION}"

curl -fLO "${BASE_URL}/${ARCHIVE}"
curl -fLO "${BASE_URL}/${ARCHIVE}.sha256"
shasum -a 256 -c "${ARCHIVE}.sha256" # macOS
# sha256sum -c "${ARCHIVE}.sha256"   # Linux
tar xzf "$ARCHIVE"
mkdir -p "$HOME/.local/bin"
install -m 0755 "$PACKAGE/dalo" "$HOME/.local/bin/dalo"
```

Do not install an archive when checksum verification fails. Verify the result
with `$HOME/.local/bin/dalo --version`.

## Shell Completions and Man Page

Each release archive contains generated Bash, Zsh, and Fish completions plus a
`dalo(1)` man page. Install only the files used by the local shell:

```sh
mkdir -p "$HOME/.local/share/bash-completion/completions"
install -m 0644 "$PACKAGE/completions/dalo.bash" \
  "$HOME/.local/share/bash-completion/completions/dalo"

mkdir -p "$HOME/.zfunc"
install -m 0644 "$PACKAGE/completions/_dalo" "$HOME/.zfunc/_dalo"

mkdir -p "$HOME/.config/fish/completions"
install -m 0644 "$PACKAGE/completions/dalo.fish" \
  "$HOME/.config/fish/completions/dalo.fish"

mkdir -p "$HOME/.local/share/man/man1"
install -m 0644 "$PACKAGE/man/man1/dalo.1" \
  "$HOME/.local/share/man/man1/dalo.1"
```

For Zsh, ensure `$HOME/.zfunc` is in `fpath` before the shell runs `compinit`.
Do not modify shell startup files without the user's confirmation.

For a Cargo or source install, generate the same files with
`dalo completions <bash|zsh|fish>` and `dalo manpage`.

## Upgrades and Removal

Upgrade by repeating the original installation method: `brew upgrade dalo`, a
fresh hosted-installer run, `npm update --global getdalo`,
`cargo install dalo --locked`, or a newly downloaded and verified release
archive. Dalo never updates its own executable.

Before removing the store or binary, follow the
[uninstall guide](https://github.com/sebastian-software/dalo/blob/main/docs/uninstall.md)
so owned target links and instruction blocks are cleaned up safely.

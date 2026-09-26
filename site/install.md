# Install Dalo With an Agent

You are helping a user who asked to install Dalo, the AI agent skill manager
from Sebastian Software. The official website is https://dalo.sh and the source
repository is https://github.com/sebastian-software/dalo. The npm launcher is
named `getdalo`; it runs the same Dalo binary.

Complete the normal setup with the bundled Dalo assistant available in the
user's current agent. No preinstalled Dalo skill or second skill manager is
needed. If the user explicitly wants only the CLI, respect that scope. A request
to install Dalo does not also request migration of existing skills.

Start by checking the existing binary, store, and relevant skill folders. Reuse
what is already configured. Keep the selected store explicit with `--store`
when custom paths or `DALO_STORE` are involved. Do not initialize over a broken
store, switch installation channels unnecessarily, or replace a foreign skill.
Use the current agent as the intended target when the conversation identifies
it; ask only when the agent or installation scope is ambiguous.

## Constraints

- Only install on macOS or Linux.
- Only Apple Silicon Macs are supported. If `uname -m` reports `x86_64` on
  Darwin, stop: the Intel macOS build was discontinued with Dalo 1.0, and both
  the installer and Homebrew will refuse. Tell the user that `cargo install dalo`
  is the remaining path, because it compiles from source.
- Do not modify shell startup files automatically.
- Link only the intended agent target. Reuse the target established by the request
  or conversation; ask if that choice is unresolved.
- On macOS with Homebrew, prefer the official
  `sebastian-software/tap/dalo` formula.
- Otherwise, prefer the official installer at `https://dalo.sh/install.sh`.
- The installer always verifies the release checksum and additionally verifies
  Sigstore provenance when `cosign` is available.
- Verify the binary with `dalo --version`; run `dalo doctor` after a store exists.
- Read `dalo assistant install --help` to check bundle support. Older releases may
  need a binary upgrade or the [standalone skill route](https://dalo.sh/docs/assistant.html).

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

   On NixOS or another system with Nix, build Dalo from its source and
   locked Rust dependencies:

   ```sh
   nix profile install github:sebastian-software/dalo
   ```

   The Nix package includes Git, which Dalo uses for source operations.
   Upgrade that profile entry with `nix profile upgrade dalo`.

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

   ### Installer environment variables

   - `DALO_VERSION` selects a release. Use `dalo-v<version>`, `v<version>`, or `<version>`.
   - `DALO_INSTALL_DIR` changes the binary destination (default: `~/.local/bin`).
   - `DALO_VERIFY=required` requires Sigstore provenance verification; `auto` is the default.
   - `DALO_LINUX_LIBC=gnu|musl` overrides Linux libc detection.
   - `DALO_TARGET` overrides platform detection when non-empty. Use only a
     published target: `x86_64-unknown-linux-gnu`,
     `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
     `aarch64-unknown-linux-musl`, or `aarch64-apple-darwin`. An unset or empty
     value uses detection; an unrecognized value fails when the corresponding
     release archive is fetched.

3. Ensure the install directory is on `PATH` for this session:

   ```sh
   export PATH="$HOME/.local/bin:$PATH"
   ```

4. Verify the binary and check the bundled assistant capability:

   ```sh
   dalo --version
   dalo assistant install --help
   ```

   If the capability is missing, upgrade through the existing installation
   channel or follow the standalone skill route linked above. Do not report the
   assistant as installed just because the binary works.

5. Initialize only a missing store. For an existing store, read `status --json`
   and `doctor --json` instead and resolve incompatible or malformed state first.

   ```sh
   dalo init
   ```

6. Detect available agent targets and link the intended one:

   ```sh
   dalo target detect --json
   dalo target link codex
   ```

   `codex` is an example. Use `claude`, `openclaw`, `hermes`, or `opencode` for
   those agents, or `dalo target link generic /path/to/skills` for an explicit
   directory. Reuse an existing target's configured path. Do not repoint an
   existing target or turn project-only skills into global skills by accident.

7. Install the assistant supplied by this Dalo binary and preview delivery:

   ```sh
   dalo --dry-run --json assistant install
   dalo --json assistant install
   dalo --dry-run --json sync
   dalo --json sync --check
   ```

   Ordinary terminal commands offer a missing or outdated assistant with a
   `[y/N]` question. Agent calls using `--json` do not wait for terminal input:
   inspect `dalo assistant status --json` and ask in the conversation if setup
   or updating has not already been requested. If a terminal offer already
   installed the bundle, reuse it; a `current` bundle may still need delivery.

   Run the real sync only after checking the preview. On an existing store,
   sync can affect every linked agent and fetch tracking team sources; its dry
   run does not fetch. Resolve any effects outside the requested setup before
   applying them. An unmanaged `dalo` folder or foreign symlink is a conflict,
   not permission to overwrite it. Keep the working installation and explain
   the handover needed. Installing the local bundle does not itself touch targets;
   an update is immediately visible through existing links to that bundle.

   Inspect the resulting `dalo/SKILL.md` and its references in the intended
   agent's folder. The agent can read that file to continue this conversation
   immediately. Confirm native skill discovery on the next turn, reloading the
   skill list or starting a new session only if the host requires it.

8. Only if the user also asks to try a catalog, select one skill from
   [Sebastian's skill catalog](https://github.com/sebastian-software/skills.sebastian-software.com),
   then review and grant only the approval covered by the user's request:

   ```sh
   dalo source add-catalog sebastian https://github.com/sebastian-software/skills.sebastian-software.com.git
   dalo source inspect sebastian
   dalo source select sebastian effective-web
   dalo approve skill sebastian:effective-web
   dalo sync
   ```

   `dalo source inspect sebastian` lists what the catalog currently publishes.
   Select a name from that list: a name the catalog does not carry fails with
   exit `1` and prints the known ones.

9. Run a final health check and report the binary version, store, assistant
   location, and any remaining conflicts:

   ```sh
   dalo status --json
   dalo doctor --json
   ```

## Notes

- Dalo shells out to `git` for source operations, so `git` must be on `PATH`.
- Windows is not supported natively. Recommend WSL for Windows users; native
  Windows is tracked as
  [issue #830](https://github.com/sebastian-software/dalo/issues/830).
- Intel Macs are not supported. Dalo 0.16.0 is the last release with an Intel
  macOS archive; do not pin an older version to work around this.
- If the installer cannot resolve a release or checksum, stop and show the error instead of downloading from an unofficial mirror.
- If the install directory is not on `PATH`, the installer prints the exact export command for the current shell.
- To remove a cached npm binary, delete `~/.cache/dalo`; uninstall a global
  launcher with `npm uninstall --global getdalo`.

## Manual Release Archives

Use the archive matching the machine from the
[latest GitHub release](https://github.com/sebastian-software/dalo/releases/latest).
Set `VERSION` without the leading `v` and choose one of the published targets:
`aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, or
`aarch64-unknown-linux-musl`.

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

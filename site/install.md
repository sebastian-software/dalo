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

   - `DALO_VERSION` selects a release. Use `dalo-v0.6.1`, `v0.6.1`, or `0.6.1`.
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
   dalo source select sebastian github-pr-auto-review
   dalo approve skill sebastian:github-pr-auto-review
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

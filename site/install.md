# Install Dalo With an Agent

You are helping a user install Dalo, a Git-backed skill manager for AI agents.

## Constraints

- Only install on macOS or Linux.
- Do not modify shell startup files automatically.
- Do not touch real agent skill folders until the user confirms which agent target to link.
- Prefer the official installer at `https://dalo.sh/install.sh`.
- The installer always verifies the release checksum and additionally verifies
  Sigstore provenance when `cosign` is available.
- Verify the installed binary with `dalo --version` and `dalo doctor`.

## Steps

1. Detect the platform:

   ```sh
   uname -s
   uname -m
   ```

2. Install the latest release:

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
   release through its ubi backend:

   ```sh
   mise use -g ubi:sebastian-software/dalo
   ```

   If the user wants a custom location, set `DALO_INSTALL_DIR`:

   ```sh
   curl -fsSL https://dalo.sh/install.sh | DALO_INSTALL_DIR="$HOME/.local/bin" sh
   ```

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

8. Run a final health check:

   ```sh
   dalo status
   dalo doctor
   ```

## Notes

- Dalo shells out to `git` for source operations, so `git` must be on `PATH`.
- Windows is not supported yet. Recommend WSL for Windows users.
- If the installer cannot resolve a release or checksum, stop and show the error instead of downloading from an unofficial mirror.

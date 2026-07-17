# getdalo

`getdalo` is a small Node.js launcher for [Dalo](https://dalo.sh), the
Git-backed skill manager for AI agents. It downloads the matching official Dalo
release on first use, verifies its SHA-256 checksum, caches it locally, and
forwards every argument to the binary.

## Use it

Run Dalo without a global installation:

```sh
npx getdalo --version
npx getdalo init
```

Or install the launcher globally:

```sh
npm install --global getdalo
dalo --help
```

## Requirements

- Node.js 20 or newer
- macOS or Linux on x86_64 or ARM64
- `tar` on `PATH` to unpack the official release archive

The first invocation of a `getdalo` package version needs network access to
GitHub Releases. Later invocations use that version's cached binary without a
GitHub API call. Each launcher version defaults to its matching Dalo release,
so runs are reproducible and a warm cache works offline.

## Configuration

| Variable | Purpose |
| --- | --- |
| `DALO_VERSION` | Override the package version with `<version>`, `v<version>`, or `dalo-v<version>`. Use `latest` for an explicit GitHub update lookup; if that lookup fails, the newest usable cached version is used with a warning. |
| `DALO_CACHE_DIR` | Override the executable cache location (default: `~/.cache/dalo`). |
| `DALO_LINUX_LIBC` | Override Linux libc detection with `gnu` or `musl`. |

To force a fresh download, remove the selected version from `~/.cache/dalo` or
set `DALO_CACHE_DIR` to an empty directory.

The launcher tells the Rust binary whether it was invoked through npm or npx so
passive Dalo update notices can recommend the matching upgrade command. It never
updates the launcher or cached executable behind npm's back.

Release metadata and archive downloads time out after 30 seconds. Network
errors include their underlying cause and a hint for selecting an exact version.

## Security

The launcher downloads the archive and its `.sha256` file from the matching
GitHub Release, verifies the checksum, then installs the cached executable with
owner-only cache permissions. For Sigstore provenance verification or a
non-Node installation, use the [official installer](https://dalo.sh/install.sh).

For Dalo documentation, agent setup, and uninstall guidance, see
[dalo.sh](https://dalo.sh) and the [project repository](https://github.com/sebastian-software/dalo).

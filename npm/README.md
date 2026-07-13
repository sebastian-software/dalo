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

The first invocation needs network access to GitHub Releases. Later
invocations use the cached binary until you select another version or clear the
cache.

## Configuration

| Variable | Purpose |
| --- | --- |
| `DALO_VERSION` | Pin an exact GitHub release tag, for example `dalo-v0.7.0`. |
| `DALO_CACHE_DIR` | Override the executable cache location (default: `~/.cache/dalo`). |
| `DALO_LINUX_LIBC` | Override Linux libc detection with `gnu` or `musl`. |

To force a fresh download, remove the selected version from `~/.cache/dalo` or
set `DALO_CACHE_DIR` to an empty directory.

## Security

The launcher downloads the archive and its `.sha256` file from the matching
GitHub Release, verifies the checksum, then installs the cached executable with
owner-only cache permissions. For Sigstore provenance verification or a
non-Node installation, use the [official installer](https://dalo.sh/install.sh).

For Dalo documentation, agent setup, and uninstall guidance, see
[dalo.sh](https://dalo.sh) and the [project repository](https://github.com/sebastian-software/dalo).

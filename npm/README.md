# getdalo

`getdalo` is the official npm launcher for [Dalo](https://dalo.sh) by
Sebastian Software: your team's agent skills, versioned like code. It
downloads the matching official Dalo release on first use, verifies its
SHA-256 checksum, caches it locally, and forwards every argument to the binary.
The CLI and Cargo package are named `dalo`.

You can ask your coding agent to **“Install the Dalo skill manager.”** The
[agent setup guide](https://dalo.sh/install.md) covers installation of both the
CLI and its bundled assistant. No Dalo skill needs to be installed first.

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

Install `getdalo` globally before running `dalo autosync install`; one-off
`npx` launchers are temporary and are intentionally rejected for scheduled
jobs.

## Requirements

- Node.js 20 or newer
- macOS on Apple Silicon, or Linux on x86_64 or ARM64. Intel Macs are not
  supported: that build was discontinued with Dalo 1.0, and `cargo install dalo`
  compiles from source instead.
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
| `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` | Route release lookups and downloads through an authorized proxy. Lowercase variants are also supported and take precedence. |

To force a fresh download, remove the selected version from `~/.cache/dalo` or
set `DALO_CACHE_DIR` to an empty directory.

The launcher tells the Rust binary whether it was invoked through npm or npx so
the native CLI recommends the matching upgrade command. Every installation
uses GitHub release metadata through app-tracking and oss-metrics; releases
become eligible after one hour. The wrapper does not perform a second
passive check. Disabled/offline/JSON policy belongs to the native CLI; explicit
`DALO_VERSION=latest` lookups remain installer operations. The launcher never
updates itself or a cached executable behind npm's back.

Release-metadata lookups time out after 30 seconds; archive downloads use a
10-second response-header timeout and a 120-second body timeout and may be
retried up to twice on transient failures (three total attempts). Network
errors include their underlying cause and a hint for selecting an exact
version.
When invoked by npm, the launcher also honors `npm_config_proxy`,
`npm_config_https_proxy`, and `npm_config_noproxy` as fallbacks.

## Security

The launcher downloads the archive and its `.sha256` file from the matching
GitHub Release. It streams the archive with a 100 MiB limit and applies a
separate 1 KiB limit to the checksum file, computes the SHA-256 digest during
the download, and verifies it before promoting the binary into the cache. The
cached executable uses owner-only permissions. For Sigstore provenance
verification or a non-Node installation, use the
[official installer](https://dalo.sh/install.sh).

For Dalo documentation, agent setup, and uninstall guidance, see
[dalo.sh](https://dalo.sh) and the [project repository](https://github.com/sebastian-software/dalo).

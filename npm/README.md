# getdalo

Run Dalo without installing Rust:

```sh
npx getdalo --version
# or
npm install --global getdalo
```

On first use the launcher downloads the matching release archive from GitHub,
checks its SHA-256 file, caches the executable, and then forwards all arguments.
Set `DALO_VERSION` to pin a release tag and `DALO_CACHE_DIR` to choose the cache.

## One-time bootstrap publish

Version `0.6.1` is prepared for the initial manual publication that replaces
the failed CI publish. From this directory, after authenticating to npm with
an account that can publish `getdalo`, run:

```sh
npm test
npm pack --dry-run
npm publish
```

After that first publish, configure npm Trusted Publishing for GitHub Actions:

```sh
npm trust github getdalo --repo sebastian-software/dalo --file release-please.yml --allow-publish
```

The release workflow then publishes through GitHub OIDC, without an `NPM_TOKEN`.

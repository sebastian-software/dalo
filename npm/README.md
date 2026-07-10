# dalo

Run Dalo without installing Rust:

```sh
npx dalo --version
# or
npm install --global dalo
```

On first use the launcher downloads the matching release archive from GitHub,
checks its SHA-256 file, caches the executable, and then forwards all arguments.
Set `DALO_VERSION` to pin a release tag and `DALO_CACHE_DIR` to choose the cache.

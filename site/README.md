# Dalo Site

`dalo.sh` is hand-written HTML, CSS, and JavaScript: `index.html` is the
landing page, `install.md` and `install.sh` are served as-is, and `docs/`
contains the rendered copies of the repository's `docs/*.md`.

## Build

```sh
pnpm --dir site install    # once, installs the Markdown renderer
node site/build.mjs        # render docs/, stamp the version, write site/build/
node site/build.mjs --check   # fail if the checked-in output is stale
```

`build.mjs` does three things:

1. renders `docs/*.md` into `site/docs/*.html` with the site's own styles,
2. stamps the version from `Cargo.toml` into the version slots of
   `index.html` (`<span data-dalo-version>` and the JSON-LD `softwareVersion`),
   which release-please also keeps current through its `extra-files` entry,
3. assembles `site/build/`, the deployable tree, with `__DALO_LASTMOD__` in
   `sitemap.xml` replaced by the build date.

The rendered documentation and the stamped version are committed, so the site
stays deployable from a plain checkout and `--check` can prove they are current.
`site/build/` is generated and ignored.

Run the build after changing `docs/*.md`, `index.html`, or the version.

## Redeploy checklist

Before redeploying `dalo.sh`, verify the static site against the shipped repo state:

- `node site/build.mjs --check` passes.
- Target paths match `src/target.rs` and the README target table.
- Terminal transcripts match the current CLI output.
- The roadmap only lists future work; shipped features belong in feature content.
- Footer links resolve on `main`, including `README.md`, docs, issues,
  `LICENSE-MIT`, and `LICENSE-APACHE`.
- `install.sh` and `install.md` resolve from `https://dalo.sh/` and match README install guidance.
- `pnpm run render` in `video/` has refreshed `site/assets/dalo-quickstart.mp4` after video source changes.
- Runtime assets are self-hosted; the homepage makes no CDN/player request.
- `site/sitemap.xml` lists every published page.

Quick checks:

```sh
rg -n "v[0-9]+\\.[0-9]+\\.[0-9]+|~/.agents/skills|dalo team catalog add|dalo.toml|instruction" site/index.html
rg -n "version = " Cargo.toml
rg -n "Codex|OpenClaw" README.md src/target.rs site/index.html
sh -n site/install.sh
sh tests/docs.sh
```

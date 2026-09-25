# Dalo Site

`dalo.sh` is hand-written HTML, CSS, and JavaScript: `index.html` is the
landing page, `install.md`, `install.sh`, and `llms.txt` are served as-is,
`docs/` contains the rendered copies of the repository's `docs/*.md`, `spec/`
contains the versioned Portable Agent Packages pages plus its public JSON
Schema, and `news/` holds the occasional hand-written announcement page.
`llms.txt` is the plain-text index agents read instead of scraping the landing
page; `build.mjs` copies every deployable file in `site/` into `site/build/`,
so it needs no build-script entry of its own.

`index.html` loads only `home.css`; every other page loads `styles.css` (the
shared tokens, header, footer, and buttons, mirroring `home.css`) plus
`docs.css` (the reading layout). Both use system font stacks.

`news/` pages are not generated either: they need no build-script entry, only a
`sitemap.xml` entry. They reuse the documentation shell (`styles.css` plus
`docs.css`, `body class="doc-page"`, `.doc-shell` with a small side nav and its
`.doc-menu` counterpart for narrow screens), with the header and footer copied
from the `shell()` template in `build.mjs`.

## Build

```sh
pnpm --dir site install    # once, installs the Markdown renderer
node site/build.mjs        # render docs/, stamp the version, write site/build/
node site/build.mjs --check   # fail if the checked-in output is stale
```

`build.mjs` does three things:

1. renders `docs/*.md` and the published `docs/spec/*.md` sources into
   `site/docs/*.html` and `site/spec/0.1/*.html` with the site's own styles,
2. stamps the version from `Cargo.toml` into the version slots of
   `index.html` (`<span data-dalo-version>` and the JSON-LD `softwareVersion`),
   which release-please also keeps current through its `extra-files` entry,
3. assembles `site/build/`, the deployable tree, with `__DALO_LASTMOD__` in
   `sitemap.xml` replaced by the build date.

The rendered documentation and the stamped version are committed, so the site
stays deployable from a plain checkout and `--check` can prove they are current.

## Binary assets

Both binary assets are generated from checked-in Remotion sources in `video/`,
so neither is ever edited by hand:

```sh
pnpm --dir video install
pnpm --dir video run render      # site/assets/dalo-quickstart.mp4
pnpm --dir video run render:og   # site/assets/img/og.png
```

`render:og` renders the `DaloOg` still (`video/src/OgImage.tsx`) at 1200x630
in the light site design: the palette of `home.css`, the repository `logo.svg`,
and `site/assets/img/wordmark.svg`, shared with the video through
`video/src/brand.tsx`. The pages use the system font stacks, which a rendered
image cannot, so both renders embed the self-hosted Hanken Grotesk and Geist
Mono from `site/assets/fonts` instead. The landing page, every rendered
`docs/*.html`, and the `spec/` pages all reference that one image, so a refresh
covers the whole site. `tests/docs.sh` checks the PNG header against the
declared `og:image:width` and `og:image:height`.

Uploading the image as the repository's GitHub social preview is a separate,
manual step in the repository settings.
The current specification is published at `/spec/0.1/`; `/spec/` is its static
landing page. The versioned schema is downloadable at
`/spec/0.1/plugin-v1.schema.json`.
`site/build/` is generated and ignored.

Run the build after changing `docs/*.md`, `index.html`, or the version.

## Redeploy checklist

Before redeploying `dalo.sh`, verify the static site against the shipped repo state:

- `node site/build.mjs --check` passes.
- Target paths match `src/target.rs` and the README target table.
- Terminal transcripts match the current CLI output.
- Every "What's next" entry in the stability section links an open issue; shipped
  features belong in feature content.
- Footer links resolve on `main`, including `README.md`, docs, issues,
  `LICENSE-MIT`, and `LICENSE-APACHE`.
- `install.sh` and `install.md` resolve from `https://dalo.sh/` and match README install guidance.
- `llms.txt` is in `site/build/`, every link in it resolves, and the landing page
  still advertises it through `<link rel="alternate" type="text/plain">`.
- `pnpm run render` in `video/` has refreshed `site/assets/dalo-quickstart.mp4` after video source changes.
- `pnpm run render:og` in `video/` has refreshed `site/assets/img/og.png` after tagline or brand changes.
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

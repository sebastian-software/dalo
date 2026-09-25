# Homepage concepts

Six fundamentally different **light** design directions for the dalo.sh
homepage. They are exploration, not production: nothing here is deployed
(`site/build.mjs` only copies `site/`), and none of them replaces
`site/index.html` yet.

Every concept is one self-contained HTML file with inline CSS. The fonts are
self-hosted in [`fonts/`](fonts/) (latin subsets from Google Fonts, all SIL Open
Font License), so the pages make no external requests — the same rule the live
site follows. The shared content brief every concept was written against is
[`BRIEF.md`](BRIEF.md).

| # | Concept | Idea | Type |
| --- | --- | --- | --- |
| 01 | [Broadsheet](01-broadsheet/index.html) | The homepage as an editorial newspaper front page: masthead, lead story, columns, figures, letters, classifieds. | Instrument Serif, Newsreader, IBM Plex Mono |
| 02 | [Blueprint](02-blueprint/index.html) | An engineering drawing set: the pipeline as a dimensioned schematic, title block, section cuts, tolerances, bill of materials. | IBM Plex Sans / Condensed / Mono |
| 03 | [dalo(1)](03-manpage/index.html) | The homepage *is* the manual page, on an 80-column grid, with a live `less`-style status bar and pager keys. | JetBrains Mono |
| 04 | [Soft bento](04-bento/index.html) | A warm, pastel product page; every tile carries a small, realistic mini UI built from real Dalo output. | Plus Jakarta Sans, DM Mono |
| 05 | [Swiss](05-swiss/index.html) | International Typographic Style: a visible 12-column grid, huge tight type, one signal color, numerals as the graphic language. | Inter Tight, Space Mono |
| 06 | [Commit graph](06-commit-graph/index.html) | The page structure is a git history: lanes for `main`, `local/experiment`, and a pinned catalog tell "from local experiment to team standard". | Geist, Geist Mono |

Open [`index.html`](index.html) for a gallery, or any concept directly in a
browser (a plain `file://` URL works).

## Screenshots

```sh
node design/homepage-concepts/shoot.mjs            # all concepts
node design/homepage-concepts/shoot.mjs 04-bento   # one concept
```

The script needs Playwright (local or global install) and writes a desktop
hero (1440×900), a full desktop page, and a mobile hero (390×844 @2x) per
concept into [`screenshots/`](screenshots/). It also warns on horizontal
overflow.

## Before any of this ships

- Facts, commands, and numbers follow `BRIEF.md` and `docs/reference.md`;
  re-check them against the release that ships the page, and keep the
  invariants `tests/docs.sh` asserts on `site/index.html`.
- Sample data inside mockups (commit hashes, audit findings, placeholder
  repositories like `acme/agent-skills`) is illustrative and labelled as such
  where it could be mistaken for real output.
- Version strings are hard-coded to v0.17.0; production pages take the stamped
  `data-dalo-version` slot instead.

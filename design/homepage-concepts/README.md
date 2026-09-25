# Homepage concepts

Six **light** design directions for the dalo.sh homepage, second round. They
are exploration, not production: nothing here is deployed (`site/build.mjs`
only copies `site/`), and none of them replaces `site/index.html` yet.

Round one (newspaper, blueprint, man page, bento, Swiss poster, commit graph)
was too dense and leaned on historical metaphors; it stays in the Git history
at commit `5efdacb`. Round two follows the **Direction** section of
[`BRIEF.md`](BRIEF.md): a modern technology product in the league of Vercel,
Stripe, GitHub, and Linear, built around three core messages — one source of
truth, reviewed before it reaches an agent, safe by default — with details
left to the docs. Every page stays within 250–400 words of visible copy.

Every concept is one self-contained HTML file with inline CSS. The fonts are
self-hosted in [`fonts/`](fonts/) (latin subsets from Google Fonts, all SIL Open
Font License), so the pages make no external requests — the same rule the live
site follows.

| # | Concept | Idea | Type |
| --- | --- | --- | --- |
| 01 | [Monochrome](01-monochrome/index.html) | Engineered minimalism: a hairline frame with crosshair marks, a thin-line diagram from sources through Dalo to every agent, three quiet message cells. | Geist, Geist Mono |
| 02 | [Gradient](02-gradient/index.html) | Colorful polish: a slanted gradient band behind the hero, layered product cards, three alternating message rows. | Inter, JetBrains Mono |
| 03 | [Product](03-product/index.html) | Show the product: a large, crafted `dalo status` window as the hero, then zoomed product crops for each message. | Mona Sans, Geist Mono |
| 04 | [Terminal](04-terminal/index.html) | A very short page around one light terminal with Pin · Review · Sync tabs and real output (works without JS). | Hanken Grotesk, JetBrains Mono |
| 05 | [Flow](05-flow/index.html) | One spatial node graph of glass cards: sources into a glowing Dalo card with its gates, out to every agent; later sections zoom into it. | Manrope, Geist Mono |
| 06 | [Story](06-story/index.html) | Scrollytelling: huge type, one statement per screen, a sticky visual that changes with each core message. | Inter Tight, IBM Plex Mono |

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

# Dalo 1.0 announcement kit

Drafts, not posts. Nothing in this folder has been published anywhere. The
maintainer posts each item by hand, after the `dalo-v1.0.0` tag is out and the
release artifacts are downloadable, in the order below.

Every factual claim in these drafts is traceable to
[`docs/upgrading.md`](../../../docs/upgrading.md),
[`docs/compatibility.md`](../../../docs/compatibility.md),
[`docs/security.md`](../../../docs/security.md), or
[`docs/comparison.md`](../../../docs/comparison.md). If a draft and a document
disagree after later changes on `main`, the document wins and the draft is
edited before it is posted.

## Contents

| File | Channel | Angle |
| --- | --- | --- |
| [`linkedin.md`](linkedin.md) | LinkedIn | Team drift as a management problem, then the review gate, then the stability contract |
| [`x-bluesky.md`](x-bluesky.md) | X and Bluesky | Six-post thread, same text on both, each post under 280 characters |
| [`mastodon.md`](mastodon.md) | Mastodon | One post plus an optional reply; licensing, no telemetry, and the stated limits up front |
| [`hacker-news.md`](hacker-news.md) | Hacker News | Show HN title plus a first comment: mechanism, the nearest alternatives, and what it is not |
| [`reddit-r-claudeai.md`](reddit-r-claudeai.md) | r/ClaudeAI | Drift and trust inside `~/.claude/skills` for a team of three or four |
| [`reddit-r-chatgptcoding.md`](reddit-r-chatgptcoding.md) | r/ChatGPTCoding | One skill set across several agents, Codex first; team case second |
| [`reddit-r-rust.md`](reddit-r-rust.md) | r/rust | Landlock and Seatbelt, the tiered compatibility contract, and why the crate is not a library |
| [`descriptions.md`](descriptions.md) | crates.io, npm, Homebrew, GitHub, OSS site | One tagline everywhere, with the two manual steps listed |

The announcement page itself is not in this folder: it is
[`site/news/1-0.html`](../../../site/news/1-0.html), published at
`https://dalo.sh/news/1-0.html`.

## Order

The order matters more than the timing. Hacker News and Reddit come first
because they generate the questions that improve every later post, and the
paid-audience channels come last so they link to a page that has already
survived an hour of scrutiny.

1. Publish `dalo-v1.0.0` through the Release Please workflow and confirm
   crates.io, npm, and the Homebrew tap all serve 1.0.0.
2. Deploy `dalo.sh` so `/news/1-0.html` is live, and add the homepage CTA
   (snippet below).
3. Post the GitHub Discussion in Announcements, linking the page. Answer
   questions on the channel where they arrive; cross-link useful answers when
   they help another conversation.
4. Show HN, early on a weekday, US morning. Post the first comment immediately
   after submitting. Stay available for two hours.
5. r/rust, then r/ClaudeAI, then r/ChatGPTCoding, one per day, never two on the
   same day and never the same text twice.
6. Mastodon, then X and Bluesky.
7. LinkedIn last, once the page has been read and the obvious objections are
   known.

## Checklist

Before any of it:

- [ ] `dalo-v1.0.0` is tagged and the release is published, not a draft.
- [ ] `brew install sebastian-software/tap/dalo`, `npx getdalo --version`, and
      `curl -fsSL https://dalo.sh/install.sh | sh` all install 1.0.0 on a clean
      machine.
- [ ] `https://dalo.sh/news/1-0.html` is live and every link on it resolves.
- [ ] The date on the page reads the real launch month, not a placeholder.
- [ ] The descriptions in [`descriptions.md`](descriptions.md) match on
      crates.io, npm, the tap, the repository About box, and the OSS listing.
- [ ] Every number in a draft has been re-verified against `main` at the tag,
      not against the day the draft was written. The facts that were true when
      this kit was written: 34 public releases since 0.1, 1011 tests behind an
      86.9% line-coverage gate, five signed release targets, six install
      channels, and four findings from the 2026-09 audit round, all closed
      (#712, #713, #714, #715). The release and test counts move; the rest are
      contractual.
- [ ] Discussions is open and the Announcements category exists.

Per post:

- [ ] Posted from the maintainer account, not an org account, where the
      community expects a person.
- [ ] Subreddit rules and flair checked; self-promotion rules differ per sub.
- [ ] Character counts re-checked for X, Bluesky, and Mastodon after any edit.
- [ ] Comments answered for the first few hours. An unanswered Show HN or
      Reddit thread is worse than not posting.

## Homepage CTA

`site/index.html` is not touched by this pull request. Add one line to the
hero, after the `hero-next` paragraph, during launch week, and remove it again
when it stops earning its place:

```html
<p class="hero-next reveal"><a href="/news/1-0.html">Read the 1.0 announcement</a></p>
```

## harness-relay cross-link

Checked with
`gh api repos/sebastian-software/harness-relay/readme --jq .content | base64 -d | grep -i dalo`:
harness-relay's README **already links back to Dalo**. Its "two halves of one
story" paragraph describes Dalo as distributing the skills an agent runs and
links `https://github.com/sebastian-software/dalo`. No snippet is needed and no
pull request should be opened on that repository.

Two things worth doing there anyway, both the maintainer's call:

- Point the link at `https://dalo.sh` rather than the repository, so the
  cross-link lands on the page that explains the tool.
- Mention the 1.0 release in the harness-relay announcement, if one follows,
  rather than editing its README for a version bump on another project.

Dalo's own [README "Related" section](../../../README.md#related) already links
harness-relay, so the pair is discoverable in both directions today. Note that
`README.md` is generated from `README.md.src` by mdtheme; edit the source.

## Two weeks after launch

No tracking issue is created for this. Two weeks after the last post, spend an
hour collecting the following and turning it into changes, then close the loop
in the Discussion thread.

**What to collect**

- Every question asked in Discussions, on Reddit, on Hacker News, and in
  replies, with the answer that was actually given. A question asked twice is
  a documentation gap, not a support request.
- Every install that failed, with the platform, the channel, and the error.
  Distinguish "the installer broke" from "WSL was expected to be native
  Windows".
- Which claim got challenged. The candidates are the review gate (does a
  preflight that reads without executing buy anything), the pinning argument
  (why a commit rather than a tag), the sandbox scope (it covers generated
  delivery, not hooks), and the library stance.
- Which comparison came up unprompted, and whether
  [`docs/comparison.md`](../../../docs/comparison.md) still describes those
  tools correctly at their current versions.
- Anything a reader believed that is not true. That is a copy bug on the page
  or in a draft, and it is the most valuable thing on this list.

**Where it feeds**

- Repeated questions become entries in the FAQ section of
  [`docs/troubleshooting.md`](../../../docs/troubleshooting.md), in the wording
  the asker used rather than the wording the docs use.
- Challenged claims become either a sharper sentence on
  `site/index.html`'s trust facts row, each still linking to where the number
  can be checked, or a removed claim. A claim that cannot be checked in one
  click does not belong on the page.
- Install failures become issues with the platform in the title, and, if the
  same platform appears twice, a line in
  [`docs/compatibility.md`](../../../docs/compatibility.md).
- An out-of-date comparison row is a pull request against
  `docs/comparison.md`, with the snapshot date and the versions updated.

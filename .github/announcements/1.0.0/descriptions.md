# One-line descriptions for 1.0

Every distribution channel currently shows a different sentence. For 1.0 they
all say the same thing, and that thing is the tagline the README and the site
hero already lead with:

> **Your team's agent skills, versioned like code.**

It is 46 characters, fits every field below without truncation, and is the only
sentence a reader may meet three times in one search result, so it is worth
having it be identical each time.

## The table

| Channel | Field | Before | After | Changed in this PR |
| --- | --- | --- | --- | --- |
| crates.io | `Cargo.toml` `description` | One source of truth for the skills your AI agents run. | Your team's agent skills, versioned like code. | Yes |
| npm (`getdalo`) | `npm/package.json` `description` | One source of truth for the skills your AI agents run. | Your team's agent skills, versioned like code. | Yes |
| Homebrew tap | `Formula/dalo.rb` `desc` | Git-backed skill management for AI agents | Your team's agent skills, versioned like code | No, other repository |
| GitHub repository | About → Description | One source of truth for the skills your AI agents run. | Your team's agent skills, versioned like code. | No, repository setting |
| OSS site listing | `oss.sebastian-software.com` entry | (whatever it shows today) | Your team's agent skills, versioned like code. | No, other property |

`npm/test/release.test.js` asserts the npm description, so it was updated in the
same commit. The npm `version` fields stay untouched: release-please owns them.

## What the maintainer still has to do by hand

1. **Homebrew tap.** In `sebastian-software/homebrew-tap`, edit
   `Formula/dalo.rb` and set
   `desc "Your team's agent skills, versioned like code"`. Homebrew's audit
   rejects a `desc` that ends in a period and rejects one starting with an
   article, so the trailing period is dropped there and only there.
2. **GitHub repository description.** Settings, or
   `gh repo edit sebastian-software/dalo --description "Your team's agent skills, versioned like code."`.
   The homepage stays `https://dalo.sh`.
3. **OSS site listing** on `oss.sebastian-software.com`: same sentence, same
   punctuation as crates.io and npm.

crates.io and npm pick the new text up with the next publish, which is the
1.0.0 release itself, so nothing has to be republished for it.

## Deliberately not changed

`src/cli.rs` still carries the old sentence as the clap `about` and `long_about`
text. That string is `dalo --help` output, which is CLI surface rather than
package metadata, so changing it belongs in a behavior change with its own
review, not in an announcement PR. If it is aligned later, `about`,
`long_about`, and any help snapshots move together.

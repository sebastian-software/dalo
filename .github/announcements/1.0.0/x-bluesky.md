# X and Bluesky

One thread, six posts, each under 280 characters so the same text fits both
platforms unchanged. Post it on X and on Bluesky. Do not reuse the LinkedIn
text here.

Each post body is between a `<!-- post -->` marker and the next heading, with
no hard wrapping, so it can be copied as-is. Blank lines inside a post are
intentional line breaks. Current counts are 218, 210, 245, 245, 254, and 216.
Re-check after any edit:

```sh
python3 - <<'PY'
text = open(".github/announcements/1.0.0/x-bluesky.md").read()
for i, part in enumerate(text.split("<!-- post -->\n")[1:], 1):
    print(i, len(part.split("\n## ")[0].rstrip("\n")))
PY
```

## 1/6

<!-- post -->
Dalo 1.0 is out.

If two people on your team run Claude Code, their skill folders have already drifted apart. Dalo keeps the skills in Git and links one resolved, approved set into the folders your agents already read.

## 2/6

<!-- post -->
The agents learn nothing new. They keep reading ~/.claude/skills or ~/.agents/skills.

Dalo owns what is behind those folders: sources, priorities, approvals, conflicts, drift. It repairs them on the next sync.

## 3/6

<!-- post -->
A team repository carries a committed dalo.toml: your team's own skills plus the public catalogs everyone gets, pinned to exact commits.

A teammate runs dalo source add and dalo sync and has exactly what the manifest says. Not roughly the same.

## 4/6

<!-- post -->
Nothing lands in an agent folder unreviewed. A local preflight reads every skill, never executes one, and blocks the sync on high findings until a person approves.

It does not make third-party instructions safe. The docs say where that line is.

## 5/6

<!-- post -->
1.0 means the surface stops moving. Commands, flags, exit codes, JSON shapes and every persisted file are fixed for all of 1.x.

A store written by 0.6.0 opens with no migration. That is a test: the suite replays stores from four older released binaries.

## 6/6

<!-- post -->
macOS and Linux, single binary, MIT or Apache-2.0, no telemetry. Windows through WSL.

brew install sebastian-software/tap/dalo
curl -fsSL https://dalo.sh/install.sh | sh

Release notes: https://dalo.sh/news/1-0.html

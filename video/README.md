# Dalo quickstart video

The Remotion source in this directory renders the static quickstart video used
on `dalo.sh`. Remotion and React are build-time dependencies only; the website
ships the resulting MP4 without a JavaScript video player.

```sh
pnpm install
pnpm run studio
pnpm run render
pnpm run render:og
```

`pnpm run render` writes `site/assets/dalo-quickstart.mp4`. `pnpm run render:og`
writes the 1200x630 social card `site/assets/img/og.png` from the `DaloOg`
still, which reuses the landing page's self-hosted fonts and dark-scope colour
tokens; the whole site shares that one image.

The terminal transcript mirrors current human-readable CLI output. When any
displayed command changes, update the transcript and render the MP4 in the same
pull request; `tests/docs.sh` guards the shared security-preflight contract.

# Dalo quickstart video

The Remotion source in this directory renders the static quickstart video used
on `dalo.sh`. Remotion and React are build-time dependencies only; the website
ships the resulting MP4 without a JavaScript video player.

```sh
pnpm install
pnpm run studio
pnpm run render
```

`pnpm run render` writes `site/assets/dalo-quickstart.mp4`.

The terminal transcript mirrors current human-readable CLI output. When any
displayed command changes, update the transcript and render the MP4 in the same
pull request; `tests/docs.sh` guards the shared security-preflight contract.

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

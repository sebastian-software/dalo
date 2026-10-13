# Dalo videos

The Remotion source in this directory renders the static quickstart video used
on `dalo.sh` and the marketing promo. Remotion and React are build-time dependencies only; the website
ships the resulting MP4 without a JavaScript video player.

```sh
pnpm install
pnpm run studio
pnpm run render
pnpm run render:og
```

`pnpm run render` writes `site/assets/dalo-quickstart.mp4`. `pnpm run render:og`
writes the 1200x630 social card `site/assets/img/og.png` from the `DaloOg`
still; the whole site shares that one image. Both renders take the light
site palette, the repository logo, and the site wordmark from `src/brand.tsx`,
and embed the self-hosted Hanken Grotesk and Geist Mono from `site/assets/fonts`
in place of the site's system font stacks, which a render cannot rely on.

The terminal transcript mirrors current human-readable CLI output. When any
displayed command changes, update the transcript and render the MP4 in the same
pull request; `tests/docs.sh` guards the shared security-preflight contract.

## Promo

`DaloPromo` (`src/promo/`) is a 56-second 1920x1080 marketing film: the hook,
"Meet Dalo", a conversation with the agent, the shared team set, the
deterministic core (pins, approvals, ownership), and the "Just ask." end card.
Every scene boundary sits on a downbeat of the score; `src/promo/timeline.ts`
and `audio/compose.py` share the 86 BPM bar plan.

Product-specific scenes (the mismatched laptops, the team sync, pins,
approvals, ownership) are our own components. Generic beats come from
[snapcn](https://snapcn.dev) (MIT), a shadcn-style registry of Remotion
components: the prompt shot that opens the chat scene. Like shadcn, its source
is copied into `src/components/snap-cn` and `src/lib/snap-cn-ui` and is ours
to edit; `src/promo/snapcn-theme.ts` re-skins
every snapcn component with the site palette and fonts. Add another one with
`pnpm dlx shadcn@latest add @snapcn/<name>`, then pin any dependency it adds
to an exact version. The `@/` import alias lives in both `tsconfig.json` and
`remotion.config.ts`, because Remotion's bundler does not read tsconfig paths.

Remotion renders the picture only. The voice and the score are built outside
it, so neither model weights nor audio binaries live in Git:

```sh
sh audio/setup.sh        # uv env (Chatterbox, Kokoro), soundfonts, once
pnpm run render:promo    # score, voice, mix, picture, MP4 + SRT
```

Both write to `audio/.work/` (ignored). `render:promo` builds the cut named by
`audio/cues/team.json` and writes `audio/.work/out/dalo-promo-team.mp4` with a
matching `.srt`.

- **Voice:** Chatterbox (MIT) conditioned on a synthetic Kokoro (Apache-2.0)
  reference voice; no real person's voice is cloned. `audio/voice.py` writes
  several seeded takes per line of `audio/lines.json` to `audio/.work/takes/`;
  takes are chosen by ear and named in the cue sheet, together with their
  start time and the caption text. Generation is not bit-for-bit reproducible
  across machines, so keep the chosen takes when re-rendering.
- **Score:** an original piano piece with a string pad, written as MIDI by
  `audio/compose.py` and rendered with fluidsynth. The piano is the Salamander
  Grand Piano V3 by Alexander Holm (CC BY 3.0), the strings come from
  MuseScore General (MIT). Credit the Salamander piano wherever the film is
  published.
- **Mix:** the music ducks under the voice and the result is normalised to
  -16 LUFS with a -1.5 dBTP ceiling.

// The promo is cut to the music: 86 BPM in 4/4, so one bar lasts 240 / 86 s.
// Scene boundaries sit on downbeats; the score in music/compose.py uses the
// same bar plan.
export const FPS = 30;
export const BPM = 86;
export const BEAT = (60 / BPM) * FPS; // frames per beat
export const BAR = BEAT * 4; // frames per bar

// Frame at the start of a bar (1-based) plus an optional beat offset.
export const bar = (n: number, beat = 0) => Math.round((n - 1) * BAR + beat * BEAT);

export const scenes = {
  hook: {from: bar(1), to: bar(4)},
  meet: {from: bar(4), to: bar(6)},
  chat: {from: bar(6), to: bar(9)},
  team: {from: bar(9), to: bar(13)},
  core: {from: bar(13), to: bar(17)},
  claim: {from: bar(17), to: bar(18)},
  end: {from: bar(18), to: bar(21)},
};

export const DURATION = scenes.end.to;

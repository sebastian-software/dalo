import type {ReactNode} from 'react';
import {Easing, interpolate} from 'remotion';

// A camera over a 2D "world": each key says which world point sits at the
// centre of the frame, at what zoom and tilt. Moves between keys ease in and
// out; fast moves pick up a little blur, like a whip pan.
export type CamKey = {f: number; x: number; y: number; z: number; r?: number; ease?: (t: number) => number};

const W = 1920;
const H = 1080;

const sample = (keys: CamKey[], frame: number) => {
  if (frame <= keys[0].f) return {...keys[0], r: keys[0].r ?? 0};
  for (let i = 0; i < keys.length - 1; i++) {
    const a = keys[i];
    const b = keys[i + 1];
    if (frame <= b.f) {
      const t = interpolate(frame, [a.f, b.f], [0, 1], {easing: b.ease ?? Easing.bezier(0.7, 0, 0.2, 1)});
      // zoom in log space so it feels linear to the eye
      const z = Math.exp(Math.log(a.z) + (Math.log(b.z) - Math.log(a.z)) * t);
      return {f: frame, x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, z, r: (a.r ?? 0) + ((b.r ?? 0) - (a.r ?? 0)) * t};
    }
  }
  const last = keys[keys.length - 1];
  return {...last, r: last.r ?? 0};
};

export const cameraAt = sample;

export const Camera = ({keys, frame, children, blur = true}: {keys: CamKey[]; frame: number; children: ReactNode; blur?: boolean}) => {
  const c = sample(keys, frame);
  const p = sample(keys, frame - 1);
  // screen-space speed in px per frame
  const speed = Math.hypot((c.x - p.x) * c.z, (c.y - p.y) * c.z) + Math.abs(Math.log(c.z / p.z)) * 900;
  const b = blur ? Math.min(14, Math.max(0, speed - 18) * 0.12) : 0;
  return (
    <div
      style={{
        position: 'absolute',
        left: 0,
        top: 0,
        width: W,
        height: H,
        transformOrigin: '0 0',
        transform: `translate(${W / 2}px, ${H / 2}px) rotate(${c.r}deg) scale(${c.z}) translate(${-c.x}px, ${-c.y}px)`,
        filter: b > 0.3 ? `blur(${b.toFixed(2)}px)` : undefined,
      }}
    >
      {children}
    </div>
  );
};

import type {ReactNode} from 'react';
import {AbsoluteFill, interpolate, useCurrentFrame} from 'remotion';

import {colors, fonts} from '../../brand';
import {Camera} from '../camera';
import {MachineCard} from '../machine';
import {BAR, BEAT} from '../timeline';
import {mix, pop, ramp} from '../ui';

// "Every agent on your team has its own skills. None of them match."
// Three laptops, three drifting copies of the "same" skills.

const Stamp = ({text, frame, at, rotate = -8, tone = '#c2361f'}: {text: string; frame: number; at: number; rotate?: number; tone?: string}) => {
  const s = pop(frame, at, {damping: 11, stiffness: 260, mass: 0.6});
  if (frame < at) return null;
  return (
    <span
      style={{
        fontFamily: fonts.mono,
        fontWeight: 500,
        fontSize: 18,
        textTransform: 'uppercase',
        letterSpacing: '0.08em',
        color: tone,
        border: `2.5px solid ${tone}`,
        borderRadius: 8,
        padding: '3px 10px',
        transform: `rotate(${rotate}deg) scale(${mix(2.4, 1, s)})`,
        opacity: Math.min(1, s * 2),
        whiteSpace: 'nowrap',
        background: 'rgba(255,255,255,.85)',
      }}
    >
      {text}
    </span>
  );
};

const NotEqual = ({frame, at, x}: {frame: number; at: number; x: number}) => {
  const s = pop(frame, at, {damping: 10, stiffness: 220});
  return (
    <div
      style={{
        position: 'absolute',
        left: x - 44,
        top: 540 - 44,
        width: 88,
        height: 88,
        borderRadius: '50%',
        background: '#fff',
        border: `2px solid #f3c9bf`,
        boxShadow: '0 10px 30px -10px rgba(194, 54, 31, .45)',
        display: 'grid',
        placeItems: 'center',
        fontFamily: fonts.sans,
        fontWeight: 700,
        fontSize: 52,
        color: '#c2361f',
        transform: `scale(${s})`,
      }}
    >
      ≠
    </div>
  );
};

// matchAt: the frame where the voice says "None of them match."
export const Hook = ({matchAt}: {matchAt: number}) => {
  const f = useCurrentFrame();
  const b = (n: number) => n * BEAT; // beats from scene start
  const clash = pop(f, matchAt, {damping: 9, stiffness: 240});
  const clashing = f >= matchAt;

  // A version tag; mismatched versions flash red on "None of them match."
  const V = ({v, stamp}: {v?: string; stamp?: ReactNode}) => (
    <div style={{display: 'flex', alignItems: 'center', gap: 10}}>
      {stamp}
      {v && (
        <span
          style={{
            fontFamily: fonts.mono,
            fontWeight: 500,
            fontSize: 20,
            padding: '3px 10px',
            borderRadius: 8,
            color: clashing ? '#c2361f' : colors.ink2,
            background: clashing ? '#fde7e3' : '#fff',
            border: `1px solid ${clashing ? '#f3c9bf' : colors.line}`,
            transform: `scale(${clashing ? 1 + Math.sin(Math.min(1, clash) * Math.PI) * 0.25 : 1})`,
          }}
        >
          {v}
        </span>
      )}
    </div>
  );

  const keys = [
    {f: 0, x: 350, y: 450, z: 1.6},
    {f: b(1.3), x: 375, y: 445, z: 1.66},
    {f: b(1.8), x: 960, y: 445, z: 1.66},
    {f: b(2.6), x: 975, y: 440, z: 1.7},
    {f: b(3.1), x: 1560, y: 445, z: 1.66},
    {f: b(3.9), x: 1575, y: 440, z: 1.7},
    {f: b(5.0), x: 960, y: 470, z: 0.98},
    {f: b(9), x: 960, y: 470, z: 1.03, r: -1},
    {f: b(11.2), x: 960, y: 470, z: 1.09, r: 1},
  ];

  // bar 4 is the dominant chord: everything gets nervous, then collapses
  const shakeAmt = ramp(f, b(9.75), b(11)) * 4.5;
  const shake = (seed: number) => Math.sin(f * 1.7 + seed) * shakeAmt;
  const collapse = ramp(f, b(11.1), BAR * 3 - 1, (t) => t * t * t);

  const card = (i: number, node: ReactNode) => {
    const x = [360, 960, 1560][i];
    const tilt = [-2.5, 1.5, -1][i] * ramp(f, b(6), b(10));
    const cx = mix(x, 960, collapse);
    return (
      <div
        style={{
          position: 'absolute',
          left: cx - 280,
          top: 540 - 300,
          transform: `translate(${shake(i * 2)}px, ${shake(i * 3 + 1)}px) rotate(${tilt + shake(i) * 0.3}deg) scale(${1 - collapse * 0.85})`,
          opacity: 1 - collapse,
        }}
      >
        {node}
      </div>
    );
  };

  const outdated = '#a55a00';
  return (
    <AbsoluteFill style={{background: '#fff'}}>
      {/* faint grid paper */}
      <AbsoluteFill
        style={{
          backgroundImage: `linear-gradient(${colors.line2} 1px, transparent 1px), linear-gradient(90deg, ${colors.line2} 1px, transparent 1px)`,
          backgroundSize: '48px 48px',
          opacity: 0.7,
        }}
      />
      <Camera keys={keys} frame={f}>
        {card(
          0,
          <MachineCard
            who="alex"
            title="Alex's laptop"
            agent="Codex"
            width={560}
            rows={[
              {name: 'incident-review', right: <V v="v3" />},
              {name: 'release-notes', color: colors.violet, right: <V v="v1" stamp={<Stamp text="outdated" frame={f} at={b(8.0)} tone={outdated} />} />},
              {name: 'code-review', color: colors.sky, right: <V v="v2" />},
            ]}
          />,
        )}
        {card(
          1,
          <MachineCard
            who="sam"
            title="Sam's laptop"
            agent="Claude Code"
            width={560}
            rows={[
              {name: 'incident-review (copy)', right: <V v="v2" stamp={<Stamp text="copy" frame={f} at={b(7.6)} />} />},
              {name: 'release-notes', color: colors.violet, right: <V v="v4" />},
              {name: 'code-review-old', color: colors.sky, right: <V v="v1" />},
            ]}
          />,
        )}
        {card(
          2,
          <MachineCard
            who="mia"
            title="Mia's laptop"
            agent="Cursor"
            width={560}
            rows={[
              {name: 'incident-review', right: <V v="v1" />},
              {name: 'release-notes', color: colors.violet, strike: true, right: <V stamp={<Stamp text="missing" frame={f} at={b(8.5)} rotate={-6} />} />},
              {name: 'code-review-final-v2', color: colors.sky, right: <V v="v5" stamp={<Stamp text="final?" frame={f} at={b(9.0)} rotate={6} />} />},
            ]}
          />,
        )}
        {collapse < 0.5 && (
          <>
            <NotEqual frame={f} at={matchAt + 4} x={660} />
            <NotEqual frame={f} at={matchAt + 10} x={1260} />
          </>
        )}
      </Camera>
      {/* white flash into "Meet Dalo" */}
      <AbsoluteFill style={{background: '#fff', opacity: interpolate(collapse, [0.6, 1], [0, 1], {extrapolateLeft: 'clamp'})}} />
    </AbsoluteFill>
  );
};

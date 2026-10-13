import type {CSSProperties, ReactNode} from 'react';
import {Easing, Img, interpolate, spring} from 'remotion';

import alex from '../../../site/assets/img/personas/alex.png';
import mia from '../../../site/assets/img/personas/mia.png';
import sam from '../../../site/assets/img/personas/sam.png';
import {colors, fonts} from '../brand';
import {FPS} from './timeline';

export const personas = {alex, sam, mia};

export const shadow = '0 1px 2px rgba(11, 23, 51, .06), 0 18px 48px -12px rgba(11, 23, 51, .22)';
export const softShadow = '0 1px 2px rgba(11, 23, 51, .05), 0 8px 24px -10px rgba(11, 23, 51, .18)';

// 0 -> 1 between two frames, eased.
export const ramp = (frame: number, from: number, to: number, ease = Easing.inOut(Easing.cubic)) =>
  interpolate(frame, [from, to], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp', easing: ease});

// A spring that starts at a given frame.
export const pop = (frame: number, at: number, config: {damping?: number; stiffness?: number; mass?: number} = {}) =>
  spring({frame: frame - at, fps: FPS, config: {damping: 14, stiffness: 140, mass: 0.8, ...config}});

export const mix = (a: number, b: number, t: number) => a + (b - a) * t;

export const Card = ({children, style}: {children: ReactNode; style?: CSSProperties}) => (
  <div
    style={{
      background: '#fff',
      border: `1px solid ${colors.line}`,
      borderRadius: 22,
      boxShadow: shadow,
      ...style,
    }}
  >
    {children}
  </div>
);

export const Spark = ({size = 18, color = colors.coral, style}: {size?: number; color?: string; style?: CSSProperties}) => (
  <span style={{color, fontSize: size, lineHeight: 1, display: 'inline-block', ...style}}>✦</span>
);

export const SkillChip = ({
  name,
  color = colors.coral,
  size = 22,
  style,
  muted,
}: {
  name: ReactNode;
  color?: string;
  size?: number;
  style?: CSSProperties;
  muted?: boolean;
}) => (
  <div
    style={{
      display: 'inline-flex',
      alignItems: 'center',
      gap: size * 0.45,
      padding: `${size * 0.42}px ${size * 0.75}px`,
      borderRadius: 999,
      background: muted ? colors.surface : '#fff',
      border: `1px solid ${colors.line}`,
      boxShadow: muted ? 'none' : softShadow,
      fontFamily: fonts.mono,
      fontWeight: 500,
      fontSize: size,
      color: muted ? colors.muted : colors.ink,
      whiteSpace: 'nowrap',
      ...style,
    }}
  >
    <Spark size={size * 0.9} color={muted ? colors.faint : color} />
    {name}
  </div>
);

export const Avatar = ({who, size = 56, style}: {who: keyof typeof personas; size?: number; style?: CSSProperties}) => (
  <Img
    src={personas[who]}
    style={{width: size, height: size, borderRadius: '50%', display: 'block', flex: 'none', ...style}}
  />
);

export const Badge = ({children, tone = 'green', style}: {children: ReactNode; tone?: 'green' | 'coral' | 'violet' | 'muted' | 'red'; style?: CSSProperties}) => {
  const tones = {
    green: {bg: '#e7f6ee', fg: '#107348'},
    coral: {bg: '#fdebe4', fg: colors.rust},
    violet: {bg: '#eeeafe', fg: '#5a3fd6'},
    muted: {bg: colors.surface, fg: colors.muted},
    red: {bg: '#fde7e3', fg: '#c2361f'},
  }[tone];
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 8,
        padding: '6px 14px',
        borderRadius: 999,
        background: tones.bg,
        color: tones.fg,
        fontFamily: fonts.sans,
        fontWeight: 600,
        fontSize: 20,
        whiteSpace: 'nowrap',
        ...style,
      }}
    >
      {children}
    </span>
  );
};

export const Check = ({size = 28, color = colors.green, progress = 1}: {size?: number; color?: string; progress?: number}) => (
  <svg width={size} height={size} viewBox="0 0 36 36">
    <circle cx="18" cy="18" r="16" fill={color} opacity={0.12 * progress} />
    <path
      d="m11 18 5 5 9-10"
      fill="none"
      stroke={color}
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
      pathLength={1}
      strokeDasharray={1}
      strokeDashoffset={1 - progress}
    />
  </svg>
);

export const Ring = ({size = 44, progress}: {size?: number; progress: number}) => (
  <svg width={size} height={size} viewBox="0 0 36 36" style={{transform: 'rotate(-90deg)'}}>
    <circle cx="18" cy="18" r="14" fill="none" stroke={colors.line} strokeWidth="3" />
    <circle
      cx="18"
      cy="18"
      r="14"
      fill="none"
      stroke={colors.violet}
      strokeWidth="3"
      strokeLinecap="round"
      pathLength={100}
      strokeDasharray={100}
      strokeDashoffset={100 - progress * 100}
    />
  </svg>
);

export const LockIcon = ({size = 64, open = 0, color = colors.ink}: {size?: number; open?: number; color?: string}) => (
  <svg width={size} height={size} viewBox="0 0 64 64">
    <g transform={`translate(${44 - 0} ${26}) rotate(${-open * 38}) translate(${-44} ${-26})`} style={{}}>
      <path
        d={`M20 ${28 - open * 8} V20 a12 12 0 0 1 24 0 V${28 - open * 2}`}
        fill="none"
        stroke={color}
        strokeWidth="5"
        strokeLinecap="round"
        transform={`translate(0 ${-open * 6})`}
      />
    </g>
    <rect x="12" y="28" width="40" height="30" rx="7" fill={color} />
    <circle cx="32" cy="41" r="4" fill="#fff" />
    <rect x="30.5" y="42" width="3" height="8" rx="1.5" fill="#fff" />
  </svg>
);

export const PinIcon = ({size = 40, color = colors.coral}: {size?: number; color?: string}) => (
  <svg width={size} height={size} viewBox="0 0 24 24">
    <path d="M12 2a6 6 0 0 1 6 6c0 4.5-6 13-6 13S6 12.5 6 8a6 6 0 0 1 6-6Z" fill={color} />
    <circle cx="12" cy="8" r="2.3" fill="#fff" />
  </svg>
);

export const FolderIcon = ({size = 40, color = colors.ink2}: {size?: number; color?: string}) => (
  <svg width={size} height={size} viewBox="0 0 24 24">
    <path d="M3 6.5A1.5 1.5 0 0 1 4.5 5h4.6l2 2h8.4A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5Z" fill="none" stroke={color} strokeWidth="1.6" strokeLinejoin="round" />
  </svg>
);

export const RepoIcon = ({size = 34, color = colors.ink}: {size?: number; color?: string}) => (
  <svg width={size} height={size} viewBox="0 0 24 24">
    <path d="M5 4h13v16H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2Zm0 12h13M7 7h7M7 10h5" fill="none" stroke={color} strokeWidth="1.5" strokeLinejoin="round" />
  </svg>
);

// A point along a cubic Bézier, for the ✦ packets.
export const bezier = (
  t: number,
  p0: [number, number],
  p1: [number, number],
  p2: [number, number],
  p3: [number, number],
): [number, number] => {
  const u = 1 - t;
  const x = u * u * u * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t * t * t * p3[0];
  const y = u * u * u * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t * t * t * p3[1];
  return [x, y];
};

// Horizontal S-curve between two points, as the site draws its connectors.
export const sCurve = (a: [number, number], b: [number, number]) => {
  const dx = (b[0] - a[0]) * 0.5;
  return [a, [a[0] + dx, a[1]], [b[0] - dx, b[1]], b] as [[number, number], [number, number], [number, number], [number, number]];
};

export const curvePath = (a: [number, number], b: [number, number]) => {
  const [p0, p1, p2, p3] = sCurve(a, b);
  return `M${p0[0]} ${p0[1]} C${p1[0]} ${p1[1]} ${p2[0]} ${p2[1]} ${p3[0]} ${p3[1]}`;
};

// Big, tight headline type of the site hero.
export const headline: CSSProperties = {
  fontFamily: fonts.sans,
  fontWeight: 700,
  letterSpacing: '-0.04em',
  color: colors.ink,
  lineHeight: 1.02,
};

export const gradientText: CSSProperties = {
  backgroundImage: `linear-gradient(95deg, ${colors.coral} 5%, #d94a6e 45%, ${colors.violet} 95%)`,
  WebkitBackgroundClip: 'text',
  backgroundClip: 'text',
  color: 'transparent',
};

// Kinetic word-by-word reveal: each word rises in on its own spring.
export const Words = ({
  text,
  frame,
  at,
  stagger = 4,
  style,
  wordStyle,
}: {
  text: string;
  frame: number;
  at: number;
  stagger?: number;
  style?: CSSProperties;
  wordStyle?: (i: number) => CSSProperties;
}) => (
  <div style={{display: 'flex', flexWrap: 'wrap', gap: '0 0.26em', ...style}}>
    {text.split(' ').map((w, i) => {
      const s = pop(frame, at + i * stagger, {damping: 16, stiffness: 170});
      return (
        <span
          key={i}
          style={{
            display: 'inline-block',
            opacity: Math.min(1, s * 1.4),
            transform: `translateY(${(1 - s) * 0.5}em)`,
            filter: `blur(${(1 - Math.min(1, s)) * 8}px)`,
            ...(wordStyle ? wordStyle(i) : {}),
          }}
        >
          {w}
        </span>
      );
    })}
  </div>
);

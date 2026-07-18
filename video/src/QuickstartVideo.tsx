import type {CSSProperties, ReactNode} from 'react';
import {AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig} from 'remotion';

const colors = {
  background: '#f4f1eb',
  ink: '#ebeef5',
  dim: '#858b99',
  panel: '#181a20',
  panelTop: '#202229',
  line: '#353842',
  orange: '#ff7a45',
  green: '#5fd3a0',
  yellow: '#e9bd67',
};

type Line = {
  at: number;
  kind: 'comment' | 'command' | 'output' | 'success' | 'blank';
  content?: ReactNode;
};

const lines: Line[] = [
  {at: 20, kind: 'comment', content: '# inspect the exact skill before approval'},
  {at: 42, kind: 'command', content: <>dalo audit local:review</>},
  {at: 66, kind: 'output', content: 'security audit: local:review'},
  // The content hash below is an illustrative placeholder for the demo, not the
  // real audit output of any checked-in skill.
  {at: 88, kind: 'output', content: '  content hash: 4ba20c2d2fc180c89d2308e328fadc4b8f425f6fdaa30f4900bb152d02a66362'},
  {at: 110, kind: 'output', content: '  coverage: complete'},
  {at: 132, kind: 'success', content: '  result: clean'},
  {at: 154, kind: 'output', content: '  note: no findings means no known issue was detected; it is not a safety guarantee'},
  {at: 202, kind: 'command', content: <>dalo sync</>},
  {at: 226, kind: 'output', content: 'dalo store: /tmp/dalo/store'},
  {at: 250, kind: 'success', content: 'applied  create     /tmp/dalo/skills/review -> /tmp/dalo/store/local/skills/review'},
  {at: 278, kind: 'output', content: <span style={{fontSize: 12}}>security preflight: deterministic checks and compatible cached findings only; sync did not run an agent reviewer; passing is not a safety guarantee</span>},
  {at: 340, kind: 'comment', content: '# exit 0 · reviewed content linked'},
];

const DaloMark = () => (
  <svg viewBox="0 0 32 32" width="42" height="42" aria-hidden="true">
    <rect x="1.25" y="1.25" width="29.5" height="29.5" rx="8" fill="none" stroke="currentColor" strokeWidth="1.5" />
    <circle cx="9" cy="9.5" r="2" fill="currentColor" />
    <circle cx="9" cy="16" r="2" fill="currentColor" />
    <circle cx="9" cy="22.5" r="2" fill="currentColor" />
    <path d="M11 9.5 H17 Q22 9.5 22 16 Q22 22.5 17 22.5 H11 M11 16 H22" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" opacity="0.55" />
    <circle cx="22.5" cy="16" r="2.6" fill={colors.orange} />
  </svg>
);

const lineColor: Record<Line['kind'], string> = {
  comment: colors.dim,
  command: colors.ink,
  output: '#b8bdc9',
  success: colors.green,
  blank: colors.ink,
};

export const QuickstartVideo = () => {
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const enter = spring({frame, fps, config: {damping: 18, stiffness: 90, mass: 0.9}});
  const terminalStyle: CSSProperties = {
    opacity: interpolate(enter, [0, 1], [0, 1]),
    transform: `translateY(${interpolate(enter, [0, 1], [34, 0])}px) scale(${interpolate(enter, [0, 1], [0.975, 1])})`,
  };

  return (
    <AbsoluteFill
      style={{
        backgroundColor: colors.background,
        backgroundImage: 'radial-gradient(circle at 1px 1px, rgba(24,26,32,0.10) 1px, transparent 0)',
        backgroundSize: '28px 28px',
        color: '#181a20',
        fontFamily: 'Inter, ui-sans-serif, system-ui, sans-serif',
        padding: '54px 72px',
      }}
    >
      <div style={{position: 'absolute', top: -190, right: -120, width: 520, height: 520, borderRadius: '50%', background: 'rgba(255,122,69,0.13)', filter: 'blur(1px)'}} />
      <header style={{height: 52, display: 'flex', alignItems: 'center', gap: 13}}>
        <DaloMark />
        <span style={{fontSize: 32, fontWeight: 780, letterSpacing: '-0.04em'}}>dalo</span>
        <span style={{marginLeft: 10, color: '#62656e', fontSize: 19}}>one source of truth for agent skills</span>
        <span style={{marginLeft: 'auto', border: '1px solid rgba(24,26,32,0.16)', borderRadius: 999, padding: '7px 14px', fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 15}}>secure sync · 15 sec</span>
      </header>

      <div
        style={{
          ...terminalStyle,
          position: 'relative',
          height: 522,
          marginTop: 34,
          overflow: 'hidden',
          border: `1px solid ${colors.line}`,
          borderRadius: 18,
          background: colors.panel,
          boxShadow: '0 28px 70px rgba(24,26,32,0.24)',
        }}
      >
        <div style={{height: 54, background: colors.panelTop, borderBottom: `1px solid ${colors.line}`, display: 'flex', alignItems: 'center', padding: '0 20px', gap: 10}}>
          {[colors.orange, colors.yellow, colors.green].map((color) => <span key={color} style={{width: 12, height: 12, borderRadius: '50%', background: color}} />)}
          <span style={{marginLeft: 12, color: colors.dim, fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 16}}>first sync</span>
          <span style={{marginLeft: 'auto', color: colors.green, fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 14}}>● local</span>
        </div>

        <div style={{padding: '20px 28px', fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace', fontSize: 19, lineHeight: 1.48}}>
          {lines.map((line, index) => {
            const progress = interpolate(frame, [line.at, line.at + 8], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'});
            return (
              <div
                key={index}
                style={{
                  height: 28,
                  color: lineColor[line.kind],
                  opacity: progress,
                  transform: `translateY(${interpolate(progress, [0, 1], [6, 0])}px)`,
                  whiteSpace: 'pre',
                }}
              >
                {line.kind === 'command' ? <><span style={{color: colors.orange}}>$</span>{' '}</> : null}
                {line.content}
              </div>
            );
          })}
          <span style={{display: 'inline-block', width: 10, height: 22, marginTop: 3, background: colors.orange, opacity: frame % 24 < 13 ? 1 : 0}} />
        </div>
      </div>
    </AbsoluteFill>
  );
};

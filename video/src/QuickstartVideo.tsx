import type {CSSProperties, ReactNode} from 'react';
import {AbsoluteFill, interpolate, spring, useCurrentFrame, useVideoConfig} from 'remotion';

import {colors, fontFaces, fonts, Logo, Mesh, useFontsReady, Wordmark} from './brand';

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
  {at: 250, kind: 'output', content: 'target[generic]: /tmp/dalo/skills'},
  {at: 278, kind: 'success', content: 'applied  create     target[generic]:/review -> store:/local/skills/review'},
  {at: 306, kind: 'output', content: 'synced: 1 skill across 1 target (1 created)'},
  {at: 334, kind: 'output', content: 'security preflight: deterministic checks only'},
  {at: 382, kind: 'comment', content: '# exit 0 · reviewed content linked'},
];

const lineColor: Record<Line['kind'], string> = {
  comment: colors.faint,
  command: colors.ink,
  output: colors.ink2,
  success: colors.green,
  blank: colors.ink,
};

// The light terminal of the homepage hero, typing the audit-then-sync flow.
export const QuickstartVideo = () => {
  useFontsReady();
  const frame = useCurrentFrame();
  const {fps} = useVideoConfig();
  const enter = spring({frame, fps, config: {damping: 18, stiffness: 90, mass: 0.9}});
  const terminalStyle: CSSProperties = {
    opacity: interpolate(enter, [0, 1], [0, 1]),
    transform: `translateY(${interpolate(enter, [0, 1], [34, 0])}px) scale(${interpolate(enter, [0, 1], [0.975, 1])})`,
  };

  return (
    <AbsoluteFill style={{backgroundColor: colors.background, color: colors.ink, fontFamily: fonts.sans, padding: '50px 72px'}}>
      <style>{fontFaces}</style>

      {/* The hero's gradient band, washed out behind the terminal. */}
      <div style={{position: 'absolute', inset: 0, clipPath: 'polygon(0 0, 100% 0, 100% 58%, 0 86%)', opacity: 0.85}}>
        <Mesh />
        <div
          style={{
            position: 'absolute',
            inset: 0,
            background: 'linear-gradient(90deg, rgba(255,255,255,.92) 0%, rgba(255,255,255,.6) 40%, rgba(255,255,255,.1) 75%, rgba(255,255,255,0) 100%)',
          }}
        />
      </div>

      <header style={{position: 'relative', height: 52, display: 'flex', alignItems: 'center', gap: 14}}>
        <Logo height={44} />
        <Wordmark height={20} />
        <span style={{marginLeft: 12, color: colors.muted, fontSize: 20}}>one source of truth for agent skills</span>
        <span
          style={{
            marginLeft: 'auto',
            background: 'rgba(255,255,255,.82)',
            boxShadow: '0 0 0 1px rgba(11,23,51,.09)',
            borderRadius: 999,
            padding: '7px 14px',
            fontFamily: fonts.mono,
            fontSize: 15,
            color: colors.ink2,
          }}
        >
          secure sync · 15 sec
        </span>
      </header>

      <div
        style={{
          ...terminalStyle,
          position: 'relative',
          height: 530,
          marginTop: 34,
          overflow: 'hidden',
          borderRadius: 16,
          background: '#ffffff',
          boxShadow:
            '0 0 0 1px rgba(11,23,51,.07), 0 2px 4px rgba(11,23,51,.04), 0 24px 48px -16px rgba(11,23,51,.20), 0 64px 120px -40px rgba(50,50,110,.34)',
        }}
      >
        <div style={{height: 50, background: '#fbfcfd', borderBottom: `1px solid ${colors.line2}`, display: 'flex', alignItems: 'center', padding: '0 20px', gap: 9}}>
          {[0, 1, 2].map((dot) => (
            <span key={dot} style={{width: 12, height: 12, borderRadius: '50%', background: '#e4e7ee'}} />
          ))}
          <span style={{flex: 1, textAlign: 'center', color: colors.faint, fontSize: 16, marginRight: 60}}>first sync</span>
          <span style={{color: colors.green, fontFamily: fonts.mono, fontSize: 14}}>● local</span>
        </div>

        <div style={{padding: '22px 28px', fontFamily: fonts.mono, fontSize: 18.5, lineHeight: 1.5, fontVariantLigatures: 'none'}}>
          {lines.map((line, index) => {
            const progress = interpolate(frame, [line.at, line.at + 8], [0, 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'});
            return (
              <div
                key={index}
                style={{
                  height: 28,
                  color: lineColor[line.kind],
                  fontWeight: line.kind === 'command' ? 500 : 400,
                  opacity: progress,
                  transform: `translateY(${interpolate(progress, [0, 1], [6, 0])}px)`,
                  whiteSpace: 'pre',
                }}
              >
                {line.kind === 'command' ? (
                  <>
                    <span style={{color: colors.coral}}>$</span>{' '}
                  </>
                ) : null}
                {line.content}
              </div>
            );
          })}
          <span style={{display: 'inline-block', width: 10, height: 21, marginTop: 4, borderRadius: 1, background: colors.coral, opacity: frame % 24 < 13 ? 1 : 0}} />
        </div>
      </div>
    </AbsoluteFill>
  );
};

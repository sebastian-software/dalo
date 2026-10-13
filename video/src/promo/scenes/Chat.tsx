import type {ReactNode} from 'react';
import {AbsoluteFill, useCurrentFrame} from 'remotion';

import {colors, fonts, Logo} from '../../brand';
import {Camera} from '../camera';
import {BEAT} from '../timeline';
import {Badge, Card, Check, mix, pop, ramp, Ring, Spark} from '../ui';

// "Dalo finds them, checks them, and shows you exactly what changed."
// A neutral chat with "your agent": a request, the steps, a clear result.

const PROMPT = "Add our team's skills.";

const Step = ({frame, at, children}: {frame: number; at: number; children: ReactNode}) => {
  const s = pop(frame, at - 10, {damping: 18});
  const done = ramp(frame, at, at + 10);
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: 16,
        opacity: Math.min(1, s * 1.5),
        transform: `translateX(${(1 - s) * 30}px)`,
        fontFamily: fonts.sans,
        fontSize: 28,
        color: colors.ink2,
      }}
    >
      <div style={{width: 40, height: 40, position: 'relative'}}>
        <div style={{position: 'absolute', inset: 0, opacity: 1 - done}}>
          <Ring size={40} progress={ramp(frame, at - 10, at)} />
        </div>
        <div style={{position: 'absolute', inset: -2, opacity: done}}>
          <Check size={44} progress={done} />
        </div>
      </div>
      {children}
    </div>
  );
};

const ResultRow = ({
  frame,
  at,
  sign,
  name,
  color,
  detail,
  badge,
}: {
  frame: number;
  at: number;
  sign: string;
  name: string;
  color: string;
  detail?: ReactNode;
  badge: ReactNode;
}) => {
  const s = pop(frame, at, {damping: 15, stiffness: 180});
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        padding: '18px 22px',
        borderRadius: 16,
        background: colors.surface,
        border: `1px solid ${colors.line2}`,
        opacity: Math.min(1, s * 1.5),
        transform: `translateY(${(1 - s) * 40}px) scale(${mix(0.94, 1, s)})`,
      }}
    >
      <div style={{display: 'flex', alignItems: 'center', gap: 14}}>
        <span style={{fontFamily: fonts.mono, fontSize: 26, color, width: 24, textAlign: 'center'}}>{sign}</span>
        <Spark size={22} color={color} />
        <span style={{fontFamily: fonts.mono, fontSize: 26, fontWeight: 500, color: colors.ink}}>{name}</span>
        {detail && <span style={{fontFamily: fonts.mono, fontSize: 22, color: colors.muted}}>{detail}</span>}
      </div>
      {badge}
    </div>
  );
};

export const Chat = () => {
  const f = useCurrentFrame();
  const b = (n: number) => n * BEAT;

  const typed = Math.floor(ramp(f, 4, b(1.4), (t) => t) * PROMPT.length);
  const sent = f >= b(1.6);
  const userMsg = pop(f, b(1.6), {damping: 15});
  const reply = pop(f, b(2.4), {damping: 16});
  const result = pop(f, b(5.3), {damping: 17, stiffness: 120});
  const summary = pop(f, b(8.2), {damping: 12, stiffness: 160});

  const keys = [
    {f: 0, x: 710, y: 830, z: 1.75},
    {f: b(1.5), x: 715, y: 820, z: 1.7},
    {f: b(2.6), x: 760, y: 560, z: 1.02},
    {f: b(4.9), x: 740, y: 545, z: 1.08},
    {f: b(5.9), x: 1560, y: 560, z: 1.18},
    {f: b(9.5), x: 1570, y: 555, z: 1.24},
    {f: b(11.2), x: 1150, y: 545, z: 0.78},
    {f: b(16), x: 1150, y: 540, z: 0.74},
  ];

  return (
    <AbsoluteFill style={{background: colors.surface}}>
      <AbsoluteFill
        style={{
          backgroundImage: `radial-gradient(${colors.line} 1.4px, transparent 1.4px)`,
          backgroundSize: '36px 36px',
          opacity: 0.8,
        }}
      />
      <Camera keys={keys} frame={f}>
        {/* the chat */}
        <Card style={{position: 'absolute', left: 160, top: 130, width: 1100, height: 820, padding: 0, overflow: 'hidden'}}>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 16,
              padding: '26px 34px',
              borderBottom: `1px solid ${colors.line2}`,
              fontFamily: fonts.sans,
              fontWeight: 700,
              fontSize: 28,
              color: colors.ink,
            }}
          >
            <div
              style={{
                width: 46,
                height: 46,
                borderRadius: 14,
                background: `linear-gradient(135deg, ${colors.peach}, ${colors.violet})`,
                display: 'grid',
                placeItems: 'center',
                color: '#fff',
                fontSize: 24,
              }}
            >
              ✦
            </div>
            Your agent
            <span style={{marginLeft: 'auto', fontWeight: 500, fontSize: 20, color: colors.faint}}>Codex · Claude Code · Cursor · …</span>
          </div>

          <div style={{padding: '34px 40px', display: 'flex', flexDirection: 'column', gap: 28}}>
            {sent && (
              <div
                style={{
                  alignSelf: 'flex-end',
                  background: colors.ink,
                  color: '#fff',
                  fontFamily: fonts.sans,
                  fontSize: 32,
                  fontWeight: 500,
                  padding: '18px 28px',
                  borderRadius: '26px 26px 8px 26px',
                  transform: `translateY(${(1 - userMsg) * 60}px) scale(${mix(0.9, 1, userMsg)})`,
                  transformOrigin: '100% 100%',
                }}
              >
                {PROMPT}
              </div>
            )}
            {f >= b(2.4) && (
              <div
                style={{
                  display: 'flex',
                  gap: 18,
                  opacity: Math.min(1, reply * 1.5),
                  transform: `translateY(${(1 - reply) * 40}px)`,
                }}
              >
                <div style={{flex: 'none', marginTop: 2}}>
                  <Logo height={44} />
                </div>
                <div style={{display: 'flex', flexDirection: 'column', gap: 18}}>
                  <div style={{fontFamily: fonts.sans, fontSize: 30, color: colors.ink}}>On it — using Dalo.</div>
                  <Step frame={f} at={b(3.1)}>
                    Found 3 skills in <b style={{color: colors.ink}}>acme/agent-skills</b>
                  </Step>
                  <Step frame={f} at={b(4.0)}>
                    Checked against 15 security rules · clean
                  </Step>
                  <Step frame={f} at={b(4.6)}>
                    Linked into your agents
                  </Step>
                </div>
              </div>
            )}
          </div>

          {/* composer */}
          <div
            style={{
              position: 'absolute',
              left: 34,
              right: 34,
              bottom: 30,
              height: 84,
              borderRadius: 999,
              border: `1px solid ${colors.line}`,
              background: '#fff',
              display: 'flex',
              alignItems: 'center',
              padding: '0 30px',
              gap: 16,
              fontFamily: fonts.sans,
              fontSize: 30,
              color: typed && !sent ? colors.ink : colors.faint,
              boxShadow: '0 8px 24px -14px rgba(11,23,51,.25)',
            }}
          >
            <Spark size={26} color={colors.violet} />
            {sent ? 'Ask your agent…' : PROMPT.slice(0, typed) || 'Ask your agent…'}
            {!sent && <span style={{width: 3, height: 34, background: colors.violet, opacity: Math.floor(f / 8) % 2 ? 0 : 1}} />}
            <div
              style={{
                marginLeft: 'auto',
                width: 52,
                height: 52,
                borderRadius: '50%',
                background: typed === PROMPT.length && !sent ? colors.ink : colors.line,
                color: '#fff',
                display: 'grid',
                placeItems: 'center',
                fontSize: 26,
                transform: `scale(${f > b(1.45) && f < b(1.7) ? 0.85 : 1})`,
              }}
            >
              ↑
            </div>
          </div>
        </Card>

        {/* the result */}
        <Card
          style={{
            position: 'absolute',
            left: 1340,
            top: 200,
            width: 820,
            padding: 36,
            opacity: Math.min(1, result * 1.4),
            transform: `translateX(${(1 - result) * 160}px) rotate(${(1 - result) * 4}deg)`,
          }}
        >
          <div style={{display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 24}}>
            <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 36, color: colors.ink, letterSpacing: '-0.02em'}}>What changed</div>
            <Badge tone="muted">Codex · Claude Code</Badge>
          </div>
          <div style={{display: 'flex', flexDirection: 'column', gap: 14}}>
            <ResultRow frame={f} at={b(5.8)} sign="+" name="incident-review" color={colors.coral} badge={<Badge>Added</Badge>} />
            <ResultRow frame={f} at={b(6.3)} sign="+" name="release-notes" color={colors.violet} badge={<Badge>Added</Badge>} />
            <ResultRow frame={f} at={b(6.8)} sign="↻" name="code-review" color={colors.sky} detail="v3 → v4" badge={<Badge tone="violet">Updated</Badge>} />
            <ResultRow frame={f} at={b(7.4)} sign="·" name="my-notes/" color={colors.faint} badge={<Badge tone="muted">Your files · untouched</Badge>} />
          </div>
          <div
            style={{
              marginTop: 26,
              display: 'flex',
              alignItems: 'center',
              gap: 14,
              padding: '16px 22px',
              borderRadius: 16,
              background: '#e7f6ee',
              color: '#107348',
              fontFamily: fonts.sans,
              fontWeight: 600,
              fontSize: 28,
              opacity: Math.min(1, summary * 1.5),
              transform: `scale(${mix(0.85, 1, summary)})`,
            }}
          >
            <Check size={36} progress={ramp(f, b(8.2), b(8.8))} />2 added · 1 updated · your files untouched
          </div>
        </Card>
      </Camera>
    </AbsoluteFill>
  );
};

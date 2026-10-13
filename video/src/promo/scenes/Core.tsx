import type {ReactNode} from 'react';
import {AbsoluteFill, useCurrentFrame} from 'remotion';

import {colors, fonts, Logo} from '../../brand';
import {Camera} from '../camera';
import {BAR, BEAT} from '../timeline';
import {
  Avatar,
  Badge,
  Card,
  Check,
  FolderIcon,
  gradientText,
  headline,
  LockIcon,
  mix,
  PinIcon,
  pop,
  ramp,
  Spark,
  Words,
} from '../ui';

// "Underneath the conversation sits a deterministic core. Every version is
// pinned. New skills wait for your approval. And Dalo only touches what it owns."
// One bar per idea; the camera whips from panel to panel on the downbeat.

const PANEL = 2400; // world distance between panels
const px = (k: number) => 960 + k * PANEL;

const Tile = ({frame, at, icon, label}: {frame: number; at: number; icon: ReactNode; label: string}) => {
  const s = pop(frame, at, {damping: 12, stiffness: 180});
  return (
    <Card
      style={{
        width: 300,
        height: 240,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 22,
        transform: `translateY(${(1 - s) * 80}px) scale(${mix(0.8, 1, s)})`,
        opacity: Math.min(1, s * 1.5),
      }}
    >
      {icon}
      <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 34, color: colors.ink, letterSpacing: '-0.02em'}}>{label}</div>
    </Card>
  );
};

const Title = ({k, frame, at, children}: {k: number; frame: number; at: number; children: string}) => (
  <div style={{position: 'absolute', left: px(k) - 800, top: 150, width: 1600, display: 'flex', justifyContent: 'center'}}>
    <Words text={children} frame={frame} at={at} stagger={3} style={{...headline, fontSize: 76, justifyContent: 'center'}} />
  </div>
);

export const Core = () => {
  const f = useCurrentFrame();
  const b = (n: number) => n * BEAT;
  const B = (n: number, beat = 0) => n * BAR + beat * BEAT; // bar n of this scene (0-based)

  const keys = [
    {f: 0, x: 960, y: 640, z: 0.9},
    {f: B(1, 0.1), x: 960, y: 560, z: 1.02},
    {f: B(1, 0.8), x: px(1), y: 520, z: 1.22},
    {f: B(1, 3.5), x: px(1), y: 520, z: 1.28},
    {f: B(2, 0.35), x: px(2), y: 530, z: 1.3},
    {f: B(2, 3.4), x: px(2), y: 530, z: 1.36},
    {f: B(3, 0.35), x: px(3), y: 520, z: 1.24},
    {f: B(3, 3.2), x: px(3), y: 520, z: 1.3},
    {f: B(4), x: px(3), y: 520, z: 1.7},
  ];

  // ----- pinned -----
  const p0 = B(1);
  const avail = pop(f, p0 + b(1.4), {damping: 11, stiffness: 200});
  const move = pop(f, p0 + b(2.4), {damping: 13, stiffness: 120});
  const pinX = mix(-300, 300, move);

  // ----- approval -----
  const a0 = B(2);
  const approach = ramp(f, a0 + b(0.2), a0 + b(1.0), (t) => t * t);
  const bump = f > a0 + b(1.0) ? Math.sin((f - a0 - b(1.0)) * 0.9) * Math.exp(-(f - a0 - b(1.0)) * 0.18) * 18 : 0;
  const press = pop(f, a0 + b(1.7), {damping: 9, stiffness: 300});
  const open = ramp(f, a0 + b(2.0), a0 + b(2.4));
  const through = ramp(f, a0 + b(2.4), a0 + b(3.2));
  const cardX = mix(-640, -260, approach) - bump + through * 560;
  const linked = pop(f, a0 + b(3.2), {damping: 12});

  // ----- ownership -----
  const o0 = B(3);
  const rows = [
    {name: 'incident-review', color: colors.coral, managed: true, at: o0 + b(0.6)},
    {name: 'release-notes', color: colors.violet, managed: true, at: o0 + b(1.1)},
    {name: 'my-draft/', color: colors.faint, managed: false, at: 0},
  ];

  return (
    <AbsoluteFill style={{background: colors.surface}}>
      <AbsoluteFill
        style={{
          backgroundImage: `linear-gradient(${colors.line} 1px, transparent 1px), linear-gradient(90deg, ${colors.line} 1px, transparent 1px)`,
          backgroundSize: '64px 64px',
          backgroundPosition: `${-f * 6}px 0`,
          opacity: 0.6,
        }}
      />
      <Camera keys={keys} frame={f}>
        {/* intro: the conversation lifts away, the core shows */}
        <div
          style={{
            position: 'absolute',
            left: 960 - 520,
            top: mix(380, -500, ramp(f, 0, b(2.2))),
            width: 1040,
            opacity: 1 - ramp(f, b(1), b(2.2)),
          }}
        >
          <Card style={{padding: '26px 32px', fontFamily: fonts.sans, fontSize: 32, color: colors.ink, display: 'flex', gap: 16, alignItems: 'center'}}>
            <Spark size={28} color={colors.violet} /> Add our team's skills.
            <span style={{marginLeft: 'auto'}}>
              <Check size={36} />
            </span>
          </Card>
        </div>
        <div style={{position: 'absolute', left: 0, width: 1920, top: 210, display: 'flex', flexDirection: 'column', alignItems: 'center'}}>
          <div style={{fontFamily: fonts.mono, fontSize: 26, color: colors.muted, letterSpacing: '0.12em', textTransform: 'uppercase', opacity: ramp(f, b(0.6), b(1.4))}}>
            Underneath
          </div>
          <Words text="A deterministic core." frame={f} at={b(1.2)} stagger={4} style={{...headline, fontSize: 104, marginTop: 14}} wordStyle={(i) => (i === 1 ? gradientText : {})} />
        </div>
        <div style={{position: 'absolute', left: 0, width: 1920, top: 560, display: 'flex', justifyContent: 'center', gap: 44}}>
          <Tile frame={f} at={b(2.0)} icon={<PinIcon size={72} />} label="Pinned" />
          <Tile frame={f} at={b(2.4)} icon={<LockIcon size={72} />} label="Approved" />
          <Tile frame={f} at={b(2.8)} icon={<FolderIcon size={80} color={colors.ink} />} label="Owned" />
        </div>

        {/* 1 · pinned */}
        <Title k={1} frame={f} at={p0 + b(1.1)}>
          Every version is pinned.
        </Title>
        <div style={{position: 'absolute', left: px(1) - 640, top: 380, width: 1280, height: 420}}>
          <div style={{position: 'absolute', left: 80, right: 80, top: 160, height: 6, borderRadius: 3, background: colors.line}} />
          {[
            {x: -300, label: 'v1', sub: '3f9c2e1'},
            {x: 300, label: 'v2', sub: 'a71d0b4'},
          ].map((c, i) => (
            <div key={c.label} style={{position: 'absolute', left: 640 + c.x - 90, top: 120, width: 180, textAlign: 'center'}}>
              <div style={{width: 46, height: 46, margin: '17px auto 0', borderRadius: '50%', background: '#fff', border: `6px solid ${i === 0 ? colors.ink : colors.violet}`, transform: `scale(${i === 1 ? avail : 1})`}} />
              <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 40, color: colors.ink, marginTop: 18}}>{c.label}</div>
              <div style={{fontFamily: fonts.mono, fontSize: 22, color: colors.muted}}>{c.sub}</div>
              {i === 1 && (
                <div style={{marginTop: 12, transform: `scale(${avail})`}}>
                  <Badge tone="violet">New</Badge>
                </div>
              )}
            </div>
          ))}
          {/* the team pin */}
          <div style={{position: 'absolute', left: 640 + pinX - 110, top: -20, width: 220, display: 'flex', flexDirection: 'column', alignItems: 'center'}}>
            <Badge tone="coral" style={{fontSize: 22}}>
              Team pin
            </Badge>
            <div style={{marginTop: 6, transform: `rotate(${(move - Math.min(1, move)) * 30}deg)`}}>
              <PinIcon size={84} />
            </div>
          </div>
          {/* who follows the pin */}
          <div style={{position: 'absolute', left: 0, right: 0, top: 360, display: 'flex', justifyContent: 'center', gap: 34}}>
            {(['alex', 'sam', 'mia'] as const).map((w, i) => {
              const up = pop(f, p0 + b(2.9 + i * 0.2), {damping: 12});
              return (
                <div key={w} style={{display: 'flex', alignItems: 'center', gap: 12, background: '#fff', border: `1px solid ${colors.line}`, borderRadius: 999, padding: '8px 20px 8px 8px'}}>
                  <Avatar who={w} size={48} />
                  <span style={{fontFamily: fonts.mono, fontSize: 24, color: up > 0.5 ? colors.violet : colors.ink, fontWeight: 500}}>{up > 0.5 ? 'v2' : 'v1'}</span>
                </div>
              );
            })}
          </div>
        </div>

        {/* 2 · approval */}
        <Title k={2} frame={f} at={a0 + b(0.3)}>
          New skills wait for your approval.
        </Title>
        <div style={{position: 'absolute', left: px(2) - 650, top: 400, width: 1300, height: 400}}>
          <div style={{position: 'absolute', left: 650 - 70, top: 70, display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 14, width: 140}}>
            <div style={{transform: `scale(${1 + (bump ? 0.06 : 0)})`}}>
              <LockIcon size={130} open={open} color={open > 0.5 ? colors.green : colors.ink} />
            </div>
          </div>
          <div
            style={{
              position: 'absolute',
              left: 650 - 120,
              top: 260,
              width: 240,
              height: 64,
              borderRadius: 999,
              background: open > 0.5 ? '#e7f6ee' : colors.ink,
              color: open > 0.5 ? '#107348' : '#fff',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 10,
              fontFamily: fonts.sans,
              fontWeight: 600,
              fontSize: 28,
              transform: `scale(${f > a0 + b(1.7) && f < a0 + b(2.0) ? mix(1, 0.9, press) : 1})`,
              boxShadow: f > a0 + b(1.6) && open < 0.5 ? `0 0 0 ${8 * Math.abs(Math.sin(f * 0.25))}px rgba(123,92,245,.25)` : 'none',
            }}
          >
            {open > 0.5 ? (
              <>
                <Check size={30} /> Approved
              </>
            ) : (
              'Approve'
            )}
          </div>
          <div style={{position: 'absolute', left: 650 + cardX - 190, top: 110, transform: `rotate(${bump * 0.15}deg) scale(${1 - ramp(through, 0.75, 1) * 0.3})`, opacity: 1 - ramp(through, 0.8, 1)}}>
            <Card style={{width: 380, padding: 22, borderRadius: 18}}>
              <div style={{display: 'flex', alignItems: 'center', gap: 12}}>
                <Spark size={26} color={colors.sky} />
                <span style={{fontFamily: fonts.mono, fontSize: 28, fontWeight: 500, color: colors.ink}}>effective-web</span>
              </div>
              <div style={{fontFamily: fonts.sans, fontSize: 21, color: colors.muted, marginTop: 6}}>Public catalog · checked, not yet approved</div>
            </Card>
          </div>
          <Card style={{position: 'absolute', right: 0, top: 70, width: 300, padding: 22, display: 'flex', alignItems: 'center', gap: 14}}>
            <Avatar who="alex" size={56} />
            <div>
              <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 24, color: colors.ink}}>Codex</div>
              <div style={{fontFamily: fonts.sans, fontSize: 20, color: linked > 0.5 ? '#107348' : colors.muted}}>{linked > 0.5 ? 'Linked' : 'Not linked yet'}</div>
            </div>
            <div style={{marginLeft: 'auto', opacity: linked}}>
              <Check size={36} progress={linked} />
            </div>
          </Card>
        </div>

        {/* 3 · ownership */}
        <Title k={3} frame={f} at={o0 + b(0.2)}>
          Dalo only touches what it owns.
        </Title>
        <div style={{position: 'absolute', left: px(3) - 665, top: 330, width: 1330, height: 600}}>
          <Card style={{position: 'absolute', left: 0, top: 150, width: 220, height: 200, display: 'grid', placeItems: 'center', borderRadius: 36}}>
            <Logo height={110} />
          </Card>
          <Card style={{position: 'absolute', left: 330, top: 0, width: 1000, padding: 30}}>
            <div style={{display: 'flex', alignItems: 'center', gap: 16, marginBottom: 22}}>
              <Avatar who="sam" size={56} />
              <div>
                <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 28, color: colors.ink}}>Sam's agent folder</div>
                <div style={{fontFamily: fonts.mono, fontSize: 21, color: colors.muted}}>Claude Code · ~/.claude/skills</div>
              </div>
            </div>
            <div style={{display: 'flex', flexDirection: 'column', gap: 14}}>
              {rows.map((r) => {
                const hit = r.managed ? pop(f, r.at + 16, {damping: 10, stiffness: 240}) : 0;
                return (
                  <div
                    key={r.name}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      padding: '18px 22px',
                      borderRadius: 16,
                      background: r.managed ? colors.surface : '#fff',
                      border: r.managed ? `1px solid ${colors.line2}` : `2px dashed ${colors.line}`,
                      transform: `scale(${1 + Math.sin(Math.min(1, hit) * Math.PI) * 0.03})`,
                    }}
                  >
                    <div style={{display: 'flex', alignItems: 'center', gap: 14}}>
                      {r.managed ? <Spark size={24} color={r.color} /> : <FolderIcon size={30} />}
                      <span style={{fontFamily: fonts.mono, fontSize: 27, fontWeight: 500, color: colors.ink}}>{r.name}</span>
                      <span style={{fontFamily: fonts.sans, fontSize: 20, color: colors.muted}}>{r.managed ? 'managed by Dalo' : 'your own files'}</span>
                    </div>
                    {r.managed ? (
                      f >= r.at + 16 ? (
                        <Badge tone="violet" style={{transform: `scale(${hit})`}}>
                          Updated
                        </Badge>
                      ) : (
                        <Badge tone="muted">v1</Badge>
                      )
                    ) : (
                      <Badge tone="green" style={{transform: `scale(${pop(f, o0 + b(2.0), {damping: 10})})`}}>
                        <Check size={22} /> Unchanged
                      </Badge>
                    )}
                  </div>
                );
              })}
            </div>
          </Card>
          {rows
            .filter((r) => r.managed)
            .map((r, i) => {
              const t = ramp(f, r.at, r.at + 16);
              if (f < r.at || f > r.at + 18) return null;
              const x = mix(220, 400, t);
              const y = mix(250, 158 + i * 86 + 70, t) - Math.sin(t * Math.PI) * 60;
              return <Spark key={r.name} size={38} color={r.color} style={{position: 'absolute', left: x, top: y, filter: `drop-shadow(0 0 10px ${r.color})`}} />;
            })}
        </div>
      </Camera>
    </AbsoluteFill>
  );
};

import {AbsoluteFill, useCurrentFrame} from 'remotion';

import {colors, fonts, Logo} from '../../brand';
import {Camera} from '../camera';
import {BEAT} from '../timeline';
import {
  Avatar,
  Badge,
  bezier,
  Card,
  Check,
  curvePath,
  gradientText,
  headline,
  mix,
  pop,
  ramp,
  RepoIcon,
  Ring,
  sCurve,
  SkillChip,
  Spark,
  Words,
} from '../ui';

// "Your team picks the skills once, in Git. Everyone gets the same set,
// in Codex, Claude Code, and Cursor."

const REPO: [number, number] = [330, 540];
const HUB: [number, number] = [960, 540];
const MACHINES = [
  {who: 'alex' as const, title: "Alex's laptop", agent: 'Codex', y: 250},
  {who: 'sam' as const, title: "Sam's laptop", agent: 'Claude Code', y: 540},
  {who: 'mia' as const, title: "Mia's laptop", agent: 'Cursor', y: 830},
];
const MX = 1600;
const SKILLS = [
  {name: 'incident-review', v: 'v3', color: colors.coral},
  {name: 'release-notes', v: 'v4', color: colors.violet},
];

const Packet = ({frame, at, from, to, color, dur = 18}: {frame: number; at: number; from: [number, number]; to: [number, number]; color: string; dur?: number}) => {
  if (frame < at || frame > at + dur + 4) return null;
  const t = ramp(frame, at, at + dur, (x) => (x < 0.5 ? 2 * x * x : 1 - (-2 * x + 2) ** 2 / 2));
  const [x, y] = bezier(t, ...sCurve(from, to));
  const trail = [0.06, 0.12, 0.18].map((d) => bezier(Math.max(0, t - d), ...sCurve(from, to)));
  return (
    <>
      {trail.map(([tx, ty], i) => (
        <Spark key={i} size={26 - i * 6} color={color} style={{position: 'absolute', left: tx - 10, top: ty - 14, opacity: 0.35 - i * 0.1}} />
      ))}
      <Spark size={36} color={color} style={{position: 'absolute', left: x - 14, top: y - 20, filter: `drop-shadow(0 0 10px ${color})`}} />
    </>
  );
};

export const Team = () => {
  const f = useCurrentFrame();
  const b = (n: number) => n * BEAT;

  const keys = [
    {f: 0, x: 420, y: 540, z: 1.5},
    {f: b(1.6), x: 430, y: 540, z: 1.45},
    {f: b(3.2), x: 960, y: 540, z: 0.86},
    {f: b(9.5), x: 1000, y: 540, z: 0.9},
    {f: b(11), x: 1340, y: 540, z: 1.12},
    {f: b(16), x: 1360, y: 540, z: 1.16},
  ];

  const repo = pop(f, 0, {damping: 16});
  const link = ramp(f, b(1.8), b(2.8));
  const hub = pop(f, b(1.2), {damping: 13});
  const connected = pop(f, b(3), {damping: 12, stiffness: 200});

  const sends = MACHINES.flatMap((m, i) =>
    SKILLS.map((s, j) => ({at: b(3.6 + i * 0.5 + j * 1.5), to: [MX - 270, m.y] as [number, number], color: s.color, i, j})),
  );

  return (
    <AbsoluteFill style={{background: '#fff'}}>
      <AbsoluteFill
        style={{
          backgroundImage: `linear-gradient(${colors.line2} 1px, transparent 1px), linear-gradient(90deg, ${colors.line2} 1px, transparent 1px)`,
          backgroundSize: '48px 48px',
          opacity: 0.55,
        }}
      />
      <Camera keys={keys} frame={f}>
        {/* connectors */}
        <svg width={2400} height={1200} style={{position: 'absolute', left: 0, top: 0, overflow: 'visible'}}>
          <path
            d={curvePath([REPO[0] + 250, REPO[1]], [HUB[0] - 120, HUB[1]])}
            fill="none"
            stroke={colors.line}
            strokeWidth={4}
            strokeDasharray="10 10"
            pathLength={1}
            style={{strokeDasharray: `${link} 1`}}
          />
          {MACHINES.map((m, i) => (
            <path
              key={i}
              d={curvePath([HUB[0] + 120, HUB[1]], [MX - 270, m.y])}
              fill="none"
              stroke={colors.line}
              strokeWidth={4}
              pathLength={1}
              strokeDasharray={1}
              strokeDashoffset={1 - ramp(f, b(3.0 + i * 0.25), b(3.8 + i * 0.25))}
            />
          ))}
        </svg>

        {/* the team repository */}
        <Card
          style={{
            position: 'absolute',
            left: REPO[0] - 250,
            top: REPO[1] - 200,
            width: 500,
            padding: 32,
            transform: `scale(${mix(0.8, 1, repo)})`,
            opacity: Math.min(1, repo * 1.5) * (1 - ramp(f, b(9.6), b(10.4))),
          }}
        >
          <div style={{display: 'flex', alignItems: 'center', gap: 16}}>
            <RepoIcon size={44} />
            <div>
              <div style={{fontFamily: fonts.mono, fontWeight: 500, fontSize: 28, color: colors.ink}}>acme/agent-skills</div>
              <div style={{fontFamily: fonts.sans, fontSize: 21, color: colors.muted}}>Team repository · Git</div>
            </div>
          </div>
          <div style={{display: 'flex', flexDirection: 'column', gap: 12, marginTop: 24}}>
            {SKILLS.map((s, i) => {
              const p = pop(f, b(0.6 + i * 0.5));
              return (
                <SkillChip
                  key={s.name}
                  name={<>{s.name} <span style={{color: colors.violet}}>{s.v}</span></>}
                  color={s.color}
                  size={24}
                  style={{alignSelf: 'flex-start', transform: `translateX(${(1 - p) * -40}px)`, opacity: Math.min(1, p * 1.5)}}
                />
              );
            })}
          </div>
          <div style={{marginTop: 22, fontFamily: fonts.sans, fontSize: 21, color: colors.muted}}>Chosen once. Reviewed like code.</div>
        </Card>

        {/* Dalo */}
        <div
          style={{
            position: 'absolute',
            left: HUB[0] - 130,
            top: HUB[1] - 150,
            width: 260,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            gap: 14,
            transform: `scale(${hub})`,
          }}
        >
          <Card style={{width: 220, height: 200, display: 'grid', placeItems: 'center', borderRadius: 36}}>
            <Logo height={120} />
          </Card>
          <div style={{transform: `scale(${connected})`, opacity: Math.min(1, connected * 2)}}>
            <Badge>
              <Check size={22} /> Connected
            </Badge>
          </div>
        </div>

        {/* the machines */}
        {MACHINES.map((m, i) => {
          // the voice names each agent: Codex, Claude Code, Cursor
          const named = [b(7.6), b(8.6), b(9.5)][i];
          const glow = Math.max(0, 1 - Math.abs(f - named - 6) / 14);
          const enter = pop(f, b(2.6 + i * 0.3), {damping: 15});
          const mine = sends.filter((s) => s.i === i);
          const arrived = mine.filter((s) => f >= s.at + 18).length;
          const progress = ramp(f, mine[0].at + 10, mine[mine.length - 1].at + 22, (t) => t);
          const done = ramp(f, mine[mine.length - 1].at + 22, mine[mine.length - 1].at + 32);
          return (
            <Card
              key={m.who}
              style={{
                position: 'absolute',
                left: MX - 270,
                top: m.y - 115,
                width: 640,
                height: 230,
                padding: 28,
                opacity: Math.min(1, enter * 1.5),
                transform: `translateX(${(1 - enter) * 120}px) scale(${1 + glow * 0.05})`,
                boxShadow: glow > 0 ? `0 0 0 ${glow * 6}px rgba(123, 92, 245, ${0.25 * glow}), 0 18px 48px -12px rgba(11, 23, 51, .22)` : undefined,
              }}
            >
              <div style={{display: 'flex', alignItems: 'center', gap: 16}}>
                <Avatar who={m.who} size={60} />
                <div style={{flex: 1}}>
                  <div style={{fontFamily: fonts.sans, fontWeight: 700, fontSize: 28, color: colors.ink}}>{m.title}</div>
                  <div style={{fontFamily: fonts.sans, fontSize: 21, color: colors.muted}}>{m.agent}</div>
                </div>
                <div style={{position: 'relative', width: 52, height: 52}}>
                  <div style={{position: 'absolute', inset: 0, opacity: 1 - done}}>
                    <Ring size={52} progress={progress} />
                  </div>
                  <div style={{position: 'absolute', inset: -4, opacity: done, transform: `scale(${mix(0.6, 1, done)})`}}>
                    <Check size={60} progress={done} />
                  </div>
                </div>
              </div>
              <div style={{display: 'flex', gap: 12, marginTop: 24}}>
                {SKILLS.map((s, j) => {
                  const here = j < arrived;
                  const p = pop(f, mine[j].at + 18, {damping: 11, stiffness: 220});
                  return here ? (
                    <SkillChip key={s.name} name={<>{s.name} <span style={{color: colors.violet}}>{s.v}</span></>} color={s.color} size={21} style={{transform: `scale(${p})`}} />
                  ) : (
                    <div key={s.name} style={{height: 46, width: 270, borderRadius: 999, border: `2px dashed ${colors.line}`}} />
                  );
                })}
              </div>
            </Card>
          );
        })}

        {sends.map((s, k) => (
          <Packet key={k} frame={f} at={s.at} from={[HUB[0] + 110, HUB[1]]} to={s.to} color={s.color} />
        ))}
        {/* packets from the repo into Dalo */}
        {SKILLS.map((s, j) => (
          <Packet key={j} frame={f} at={b(1.9 + j * 0.4)} from={[REPO[0] + 250, REPO[1]]} to={[HUB[0] - 110, HUB[1]]} color={s.color} />
        ))}
      </Camera>

      {/* kinetic caption */}
      <div style={{position: 'absolute', left: 90, bottom: 80, opacity: 1 - ramp(f, b(15.2), b(16))}}>
        <Words text="Same set." frame={f} at={b(10.4)} style={{...headline, fontSize: 92}} />
        <Words text="Every machine." frame={f} at={b(11)} style={{...headline, fontSize: 92}} wordStyle={() => gradientText} />
      </div>
    </AbsoluteFill>
  );
};

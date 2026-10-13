import {AbsoluteFill, useCurrentFrame} from 'remotion';

import {colors, fonts, Logo, Mesh, Wordmark} from '../../brand';
import {BEAT} from '../timeline';
import {gradientText, headline, mix, pop, ramp, Spark, Words} from '../ui';

// "Dalo. Manage your agent's setup."
export const Claim = ({manageAt}: {manageAt: number}) => {
  const f = useCurrentFrame();
  const logo = pop(f, 0, {damping: 11, stiffness: 150});
  const meshIn = ramp(f, 0, 14);
  return (
    <AbsoluteFill style={{background: '#fff', overflow: 'hidden'}}>
      <Mesh style={{opacity: meshIn, transform: `scale(${1.5 - meshIn * 0.1}) rotate(${-f * 0.05}deg)`}} />
      <AbsoluteFill style={{display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 34}}>
        <div style={{transform: `scale(${logo}) rotate(${(1 - logo) * 30}deg)`}}>
          <Logo height={210} />
        </div>
        <Words text="Manage your agent's setup." frame={f} at={manageAt} stagger={3} style={{...headline, fontSize: 104, justifyContent: 'center'}} />
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

// "Just ask." lands on the final chord, then the end card.
export const End = () => {
  const f = useCurrentFrame();
  const b = (n: number) => n * BEAT;
  const hit = pop(f, 0, {damping: 9, stiffness: 210, mass: 0.7});
  const settle = ramp(f, b(3.2), b(4.4));
  const card = pop(f, b(3.8), {damping: 16});

  const burst = Array.from({length: 14}, (_, i) => {
    const a = (i / 14) * Math.PI * 2;
    const t = ramp(f, 0, 34, (x) => 1 - (1 - x) ** 3);
    const r = mix(80, 560 + (i % 4) * 70, t);
    return (
      <Spark
        key={i}
        size={34 - (i % 4) * 6}
        color={[colors.coral, colors.violet, colors.peach, colors.sky][i % 4]}
        style={{position: 'absolute', left: 960 + Math.cos(a) * r * 1.3, top: 520 + Math.sin(a) * r * 0.75, opacity: (1 - t) * Math.min(1, t * 8)}}
      />
    );
  });

  return (
    <AbsoluteFill style={{background: '#fff', overflow: 'hidden'}}>
      <Mesh style={{opacity: 1 - settle * 0.55, transform: `scale(${1.05 + f * 0.0006})`}} />
      <AbsoluteFill style={{background: '#fff', opacity: settle * 0.45}} />
      {burst}
      <AbsoluteFill style={{display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center'}}>
        <div
          style={{
            ...headline,
            ...gradientText,
            fontSize: mix(260, 150, settle),
            paddingBottom: '0.08em',
            transform: `scale(${mix(1.6, 1, Math.min(hit, 1.2))}) translateY(${mix(0, -150, settle)}px)`,
            filter: `blur(${Math.max(0, 1 - hit * 1.4) * 14}px)`,
          }}
        >
          Just ask.
        </div>
        <div
          style={{
            position: 'absolute',
            top: 560,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            gap: 34,
            opacity: Math.min(1, card * 1.4),
            transform: `translateY(${(1 - card) * 60}px)`,
          }}
        >
          <div style={{display: 'flex', alignItems: 'center', gap: 28}}>
            <Logo height={84} />
            <Wordmark height={64} />
          </div>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 16,
              padding: '18px 34px',
              borderRadius: 999,
              background: colors.ink,
              color: '#fff',
              fontFamily: fonts.sans,
              fontWeight: 600,
              fontSize: 34,
              boxShadow: '0 20px 50px -18px rgba(11,23,51,.5)',
            }}
          >
            <Spark size={30} color={colors.peach} />
            “Install Dalo, the agent skill manager.”
          </div>
          <div style={{display: 'flex', alignItems: 'center', gap: 22, fontFamily: fonts.sans, fontSize: 30, color: colors.ink2}}>
            <span style={{fontWeight: 700, color: colors.ink, fontSize: 40, letterSpacing: '-0.02em'}}>dalo.sh</span>
            <span style={{color: colors.line}}>|</span>
            Open source · Signed releases · Built in Rust
          </div>
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

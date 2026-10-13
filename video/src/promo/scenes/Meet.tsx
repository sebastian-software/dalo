import {AbsoluteFill, interpolate, useCurrentFrame} from 'remotion';

import {colors, fonts, Logo, Mesh} from '../../brand';
import {BEAT} from '../timeline';
import {gradientText, headline, mix, pop, ramp, Spark, Words} from '../ui';

// "Meet Dalo. There's nothing new to learn. Just ask your agent."
export const Meet = () => {
  const f = useCurrentFrame();
  const b = (n: number) => n * BEAT;

  const meshIn = ramp(f, 0, 18);
  const logo = pop(f, 2, {damping: 12, stiffness: 120});
  const lift = ramp(f, b(1.5), b(2.2));
  const out = ramp(f, b(7.2), b(8), (t) => t * t);

  const burst = Array.from({length: 10}, (_, i) => {
    const a = (i / 10) * Math.PI * 2 + 0.3;
    const t = ramp(f, 4, 40, (x) => 1 - (1 - x) ** 3);
    const r = mix(40, 320 + (i % 3) * 60, t);
    return (
      <Spark
        key={i}
        size={30 - (i % 3) * 6}
        color={[colors.coral, colors.violet, colors.peach, colors.sky][i % 4]}
        style={{
          position: 'absolute',
          left: 960 + Math.cos(a) * r - 12,
          top: 470 + Math.sin(a) * r - 14,
          opacity: (1 - t) * Math.min(1, t * 6),
          transform: `rotate(${t * 90}deg)`,
        }}
      />
    );
  });

  return (
    <AbsoluteFill style={{background: '#fff', overflow: 'hidden'}}>
      <Mesh style={{opacity: meshIn * 0.9, transform: `scale(${1.45 - meshIn * 0.1}) rotate(${f * 0.03}deg)`}} />
      <AbsoluteFill style={{background: 'linear-gradient(180deg, rgba(255,255,255,0) 30%, rgba(255,255,255,.75) 100%)'}} />
      <AbsoluteFill style={{transform: `scale(${1 + out * 1.6})`, opacity: 1 - out, filter: `blur(${out * 10}px)`}}>
        {burst}
        <div
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            top: mix(300, 150, lift),
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            transform: `scale(${mix(1, 0.72, lift)})`,
          }}
        >
          <div style={{transform: `scale(${logo}) rotate(${(1 - logo) * -25}deg)`}}>
            <Logo height={230} />
          </div>
          <div style={{...headline, fontSize: 130, marginTop: 30}}>
            <Words text="Meet Dalo." frame={f} at={8} stagger={6} />
          </div>
        </div>
        <div
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            top: 620,
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            gap: 10,
            opacity: lift,
          }}
        >
          <Words
            text="Nothing new to learn."
            frame={f}
            at={b(2.0)}
            stagger={4}
            style={{...headline, fontSize: 84, color: colors.ink2, justifyContent: 'center'}}
          />
          <Words
            text="Just ask your agent."
            frame={f}
            at={b(4.5)}
            stagger={5}
            style={{...headline, fontSize: 108, justifyContent: 'center'}}
            wordStyle={(i) => (i < 2 ? gradientText : {})}
          />
        </div>
        {/* a hint of the conversation to come */}
        <div
          style={{
            position: 'absolute',
            left: 960 - 420,
            top: interpolate(pop(f, b(6.4)), [0, 1], [1120, 900]),
            width: 840,
            height: 88,
            borderRadius: 999,
            background: '#fff',
            border: `1px solid ${colors.line}`,
            boxShadow: '0 20px 50px -18px rgba(11,23,51,.35)',
            display: 'flex',
            alignItems: 'center',
            padding: '0 34px',
            fontFamily: fonts.sans,
            fontSize: 30,
            color: colors.faint,
            gap: 16,
          }}
        >
          <Spark size={28} color={colors.violet} />
          Ask your agent…
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};

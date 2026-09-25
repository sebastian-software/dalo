import {colors, fontFaces, fonts, headlineGradient, Logo, Mesh, useFontsReady, Wordmark} from './brand';

// The social card for every page of dalo.sh: the homepage hero reduced to its
// headline, the logo, and the gradient band it sits on.
export const OgImage = () => {
  useFontsReady();

  return (
    <div
      style={{
        width: 1200,
        height: 630,
        position: 'relative',
        overflow: 'hidden',
        backgroundColor: colors.background,
        color: colors.ink,
        fontFamily: fonts.sans,
      }}
    >
      <style>{fontFaces}</style>

      {/* The hero's gradient band, cut by the same slanted edge. */}
      <div style={{position: 'absolute', inset: 0, clipPath: 'polygon(0 0, 100% 0, 100% 72%, 0 100%)'}}>
        <Mesh />
        <div
          style={{
            position: 'absolute',
            inset: 0,
            background:
              'linear-gradient(90deg, rgba(255,255,255,.94) 0%, rgba(255,255,255,.82) 34%, rgba(255,255,255,.2) 62%, rgba(255,255,255,0) 78%)',
          }}
        />
      </div>

      {/* The logo, large, on a white tile floating over the gradient. */}
      <div
        style={{
          position: 'absolute',
          right: 92,
          top: 118,
          width: 300,
          height: 300,
          borderRadius: 40,
          background: '#ffffff',
          display: 'grid',
          placeItems: 'center',
          boxShadow:
            '0 0 0 1px rgba(11,23,51,.06), 0 2px 4px rgba(11,23,51,.04), 0 24px 48px -16px rgba(11,23,51,.22), 0 64px 120px -40px rgba(50,50,110,.40)',
        }}
      >
        <Logo height={196} />
      </div>

      <div style={{position: 'absolute', left: 80, top: 62, display: 'flex', alignItems: 'center', gap: 14}}>
        <Logo height={44} />
        <Wordmark height={21} />
      </div>

      <div style={{position: 'absolute', left: 80, top: 184, width: 700}}>
        <h1 style={{margin: 0, fontWeight: 700, fontSize: 68, lineHeight: 1.04, letterSpacing: '-0.045em'}}>
          Your team&rsquo;s agent setup,
          <br />
          <span style={{backgroundImage: headlineGradient, WebkitBackgroundClip: 'text', backgroundClip: 'text', color: 'transparent'}}>
            versioned like code.
          </span>
        </h1>
        <p style={{margin: '28px 0 0', fontSize: 26, lineHeight: 1.45, color: colors.ink2, maxWidth: 600}}>
          Skills, standing instructions, and hooks managed from Git.
        </p>
      </div>

      <div
        style={{
          position: 'absolute',
          left: 80,
          bottom: 56,
          display: 'flex',
          alignItems: 'center',
          gap: 14,
          fontFamily: fonts.mono,
          fontSize: 21,
          color: colors.muted,
        }}
      >
        <span style={{color: colors.rust, fontWeight: 500}}>dalo.sh</span>
        <span>·</span>
        <span>Rust CLI · MIT OR Apache-2.0 · macOS &amp; Linux</span>
      </div>
    </div>
  );
};

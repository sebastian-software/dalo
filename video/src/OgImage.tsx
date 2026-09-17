import {continueRender, delayRender} from 'remotion';
import {useEffect, useState} from 'react';

import bricolage800 from '../../site/assets/fonts/bricolage-800.woff2';
import geistMono400 from '../../site/assets/fonts/geistmono-400.woff2';
import hanken400 from '../../site/assets/fonts/hanken-400.woff2';

// The tokens below are the `[data-scope="dark"]` values from site/styles.css.
// The social card is the first thing a reader sees, so it uses the same fonts,
// colours, and wording as the hero it links to.
const colors = {
  background: 'oklch(0.185 0.012 260)',
  ink: 'oklch(0.96 0.004 240)',
  inkSoft: 'oklch(0.76 0.012 250)',
  inkFaint: 'oklch(0.62 0.012 255)',
  accent: 'oklch(0.70 0.175 45)',
  accentText: 'oklch(0.74 0.165 48)',
  line: 'oklch(0.31 0.014 260)',
};

const fontFaces = `
@font-face {
  font-family: "Bricolage Grotesque";
  font-style: normal;
  font-weight: 800;
  src: url("${bricolage800}") format("woff2");
}
@font-face {
  font-family: "Hanken Grotesk";
  font-style: normal;
  font-weight: 400;
  src: url("${hanken400}") format("woff2");
}
@font-face {
  font-family: "Geist Mono";
  font-style: normal;
  font-weight: 400;
  src: url("${geistMono400}") format("woff2");
}
`;

const DaloMark = () => (
  <svg viewBox="0 0 32 32" width="56" height="56" aria-hidden="true">
    <rect x="1.25" y="1.25" width="29.5" height="29.5" rx="8" fill="none" stroke={colors.ink} strokeWidth="1.5" />
    <circle cx="9" cy="9.5" r="2" fill={colors.ink} />
    <circle cx="9" cy="16" r="2" fill={colors.ink} />
    <circle cx="9" cy="22.5" r="2" fill={colors.ink} />
    <path
      d="M11 9.5 H17 Q22 9.5 22 16 Q22 22.5 17 22.5 H11 M11 16 H22"
      fill="none"
      stroke={colors.ink}
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      opacity="0.55"
    />
    <circle cx="22.5" cy="16" r="2.6" fill={colors.accent} />
  </svg>
);

export const OgImage = () => {
  // The still must not be captured before the self-hosted faces are ready, or
  // the headline falls back to the system sans and the card stops matching the
  // page it advertises.
  const [handle] = useState(() => delayRender('Loading the Dalo web fonts'));

  useEffect(() => {
    let cancelled = false;
    document.fonts.ready.then(() => {
      if (!cancelled) {
        continueRender(handle);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [handle]);

  return (
    <div
      style={{
        width: 1200,
        height: 630,
        position: 'relative',
        overflow: 'hidden',
        backgroundColor: colors.background,
        color: colors.ink,
        fontFamily: '"Hanken Grotesk", system-ui, sans-serif',
        padding: '64px 80px',
        boxSizing: 'border-box',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: 'space-between',
      }}
    >
      <style>{fontFaces}</style>

      {/* The hero's dotted grid and its warm corner glow. */}
      <div
        style={{
          position: 'absolute',
          inset: 0,
          backgroundImage: 'radial-gradient(circle at 1px 1px, oklch(0.96 0.004 240 / 0.09) 1px, transparent 0)',
          backgroundSize: '26px 26px',
        }}
      />
      <div
        style={{
          position: 'absolute',
          top: -260,
          right: -180,
          width: 700,
          height: 700,
          borderRadius: '50%',
          background: 'radial-gradient(circle, oklch(0.70 0.175 45 / 0.28) 0%, transparent 68%)',
        }}
      />

      <header style={{position: 'relative', display: 'flex', alignItems: 'center', gap: 18}}>
        <DaloMark />
        <span style={{fontFamily: '"Bricolage Grotesque", system-ui, sans-serif', fontWeight: 800, fontSize: 46, letterSpacing: '-0.04em'}}>
          dalo
        </span>
      </header>

      <div style={{position: 'relative'}}>
        <h1
          style={{
            fontFamily: '"Bricolage Grotesque", system-ui, sans-serif',
            fontWeight: 800,
            fontSize: 64,
            lineHeight: 1.06,
            letterSpacing: '-0.042em',
            margin: 0,
            maxWidth: 940,
          }}
        >
          Your team&rsquo;s agent skills, versioned like code.
        </h1>

        <div style={{width: 84, height: 6, borderRadius: 3, background: colors.accent, margin: '34px 0 28px'}} />

        <p style={{fontSize: 28, lineHeight: 1.44, color: colors.inkSoft, margin: 0, maxWidth: 880}}>
          Keep every skill in Git, resolve one approved set, and link it into the folders your agents already read.
        </p>
      </div>

      <footer
        style={{
          position: 'relative',
          paddingTop: 26,
          borderTop: `1px solid ${colors.line}`,
          fontFamily: '"Geist Mono", ui-monospace, Menlo, monospace',
          fontSize: 23,
          color: colors.inkFaint,
        }}
      >
        <span style={{color: colors.accentText}}>dalo.sh</span> · Rust · MIT OR Apache-2.0 · macOS &amp; Linux
      </footer>
    </div>
  );
};

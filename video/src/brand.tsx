import {continueRender, delayRender, Img} from 'remotion';
import {useEffect, useState} from 'react';
import type {CSSProperties} from 'react';

import logo from '../../logo.svg';
import wordmark from '../../site/assets/img/wordmark.svg';
import geistMono400 from '../../site/assets/fonts/geistmono-400.woff2';
import geistMono500 from '../../site/assets/fonts/geistmono-500.woff2';
import hanken400 from '../../site/assets/fonts/hanken-400.woff2';
import hanken600 from '../../site/assets/fonts/hanken-600.woff2';
import hanken700 from '../../site/assets/fonts/hanken-700.woff2';

// The palette of the light site design in site/home.css and site/styles.css.
export const colors = {
  background: '#ffffff',
  surface: '#f6f8fb',
  ink: '#0b1733',
  ink2: '#2f3c58',
  muted: '#5d6882',
  faint: '#8a93a8',
  line: '#e3e7ef',
  line2: '#eef1f6',
  coral: '#e8623a',
  rust: '#c24b22',
  peach: '#ffb58c',
  violet: '#7b5cf5',
  sky: '#43b3f2',
  green: '#16925b',
};

// The site itself uses the system UI font stacks. A rendered image cannot, so
// the card and the video embed the self-hosted grotesk and mono closest to
// that look instead of the render machine's fallback fonts.
export const fonts = {
  sans: '"Hanken Grotesk", system-ui, sans-serif',
  mono: '"Geist Mono", ui-monospace, Menlo, monospace',
};

export const fontFaces = `
@font-face { font-family: "Hanken Grotesk"; font-weight: 400; src: url("${hanken400}") format("woff2"); }
@font-face { font-family: "Hanken Grotesk"; font-weight: 600; src: url("${hanken600}") format("woff2"); }
@font-face { font-family: "Hanken Grotesk"; font-weight: 700; src: url("${hanken700}") format("woff2"); }
@font-face { font-family: "Geist Mono"; font-weight: 400; src: url("${geistMono400}") format("woff2"); }
@font-face { font-family: "Geist Mono"; font-weight: 500; src: url("${geistMono500}") format("woff2"); }
`;

// The headline gradient of the homepage hero.
export const headlineGradient = `linear-gradient(95deg, ${colors.coral} 5%, #d94a6e 45%, ${colors.violet} 95%)`;

// Frames must not be captured before the embedded faces are ready, or the
// text falls back to the render machine's sans and stops matching the site.
export const useFontsReady = () => {
  const [handle] = useState(() => delayRender('Loading the Dalo fonts'));
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
};

// The repository logo (logo.svg); its viewBox is 274 x 240.
export const Logo = ({height}: {height: number}) => (
  <Img src={logo} alt="" style={{height, width: Math.round((height * 274) / 240), display: 'block'}} />
);

// The site's drawn wordmark (site/assets/img/wordmark.svg); its viewBox is 380 x 140.
export const Wordmark = ({height}: {height: number}) => (
  <Img src={wordmark} alt="" style={{height, width: Math.round((height * 380) / 140), display: 'block'}} />
);

// Soft blurred colour fields, the same mesh the homepage hero sits on.
export const Mesh = ({style}: {style?: CSSProperties}) => (
  <div style={{position: 'absolute', inset: 0, overflow: 'hidden', background: '#fde9df', ...style}}>
    {[
      {w: 70, h: 130, l: 42, t: -55, c: 'rgba(232, 98, 58, .95)'},
      {w: 60, h: 120, l: 68, t: 5, c: 'rgba(123, 92, 245, .9)'},
      {w: 55, h: 110, l: 40, t: 35, c: 'rgba(67, 179, 242, .75)'},
      {w: 60, h: 120, l: 10, t: -40, c: 'rgba(255, 181, 140, .9)'},
      {w: 40, h: 90, l: 82, t: -40, c: 'rgba(255, 140, 110, .8)'},
    ].map((b, i) => (
      <i
        key={i}
        style={{
          position: 'absolute',
          display: 'block',
          borderRadius: '50%',
          width: `${b.w}%`,
          height: `${b.h}%`,
          left: `${b.l}%`,
          top: `${b.t}%`,
          background: `radial-gradient(closest-side, ${b.c}, transparent)`,
        }}
      />
    ))}
  </div>
);

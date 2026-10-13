import {AbsoluteFill, Sequence} from 'remotion';

import {PromptSend} from '@/components/snap-cn/prompt-send';
import {SnapCnUIProvider} from '@/lib/snap-cn-ui/theme';

import {colors, fontFaces, useFontsReady} from '../brand';
import {Chat} from './scenes/Chat';
import {Claim, End} from './scenes/Claim';
import {Core} from './scenes/Core';
import {Hook} from './scenes/Hook';
import {Meet} from './scenes/Meet';
import {Team} from './scenes/Team';
import {daloTheme} from './snapcn-theme';
import {scenes} from './timeline';

// The chat opens on snapcn's prompt shot (its own choreography, played at 2.2x
// so the click lands just before the cut), then cuts into our chat card.
const PROMPT_FRAMES = 66;

// The marketing film: picture only. The voiceover and the score are mixed and
// muxed outside Remotion (see README.md), so the render stays deterministic.
// Product-specific scenes are our own; the prompt shot that opens the chat
// comes from snapcn (src/components/snap-cn), re-skinned through the Dalo theme.
export const DaloPromo = ({matchAt = 110, manageAt = 32}: {matchAt?: number; manageAt?: number}) => {
  useFontsReady();
  const s = (k: keyof typeof scenes) => ({from: scenes[k].from, durationInFrames: scenes[k].to - scenes[k].from});
  return (
    <SnapCnUIProvider theme={daloTheme} mode="light">
      <AbsoluteFill style={{background: '#fff'}}>
        <style>{fontFaces}</style>
        <Sequence {...s('hook')}>
          <Hook matchAt={matchAt} />
        </Sequence>
        <Sequence {...s('meet')}>
          <Meet />
        </Sequence>
        <Sequence {...s('chat')}>
          <Chat />
          <Sequence durationInFrames={PROMPT_FRAMES}>
            <PromptSend
              text="Add our team's skills."
              placeholder="Ask your agent…"
              chips={[]}
              accentColor={colors.violet}
              speed={2.2}
            />
          </Sequence>
        </Sequence>
        <Sequence {...s('team')}>
          <Team />
        </Sequence>
        <Sequence {...s('core')}>
          <Core />
        </Sequence>
        <Sequence {...s('claim')}>
          <Claim manageAt={manageAt} />
        </Sequence>
        <Sequence {...s('end')}>
          <End />
        </Sequence>
      </AbsoluteFill>
    </SnapCnUIProvider>
  );
};

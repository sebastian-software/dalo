import {Composition} from 'remotion';
import {QuickstartVideo} from './QuickstartVideo';

export const VideoRoot = () => (
  <Composition
    id="DaloQuickstart"
    component={QuickstartVideo}
    durationInFrames={360}
    fps={30}
    width={1280}
    height={720}
  />
);

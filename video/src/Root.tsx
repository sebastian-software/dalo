import {Composition, Still} from 'remotion';
import {OgImage} from './OgImage';
import {QuickstartVideo} from './QuickstartVideo';

export const VideoRoot = () => (
  <>
    <Composition
      id="DaloQuickstart"
      component={QuickstartVideo}
      durationInFrames={450}
      fps={30}
      width={1280}
      height={720}
    />
    <Still id="DaloOg" component={OgImage} width={1200} height={630} />
  </>
);

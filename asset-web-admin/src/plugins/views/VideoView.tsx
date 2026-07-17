import {
  MediaControlBar,
  MediaController,
  MediaDurationDisplay,
  MediaFullscreenButton,
  MediaLoadingIndicator,
  MediaMuteButton,
  MediaPipButton,
  MediaPlayButton,
  MediaSeekBackwardButton,
  MediaSeekForwardButton,
  MediaTimeDisplay,
  MediaTimeRange,
  MediaVolumeRange,
} from "media-chrome/react";

export function VideoView({ src, title }: { src: string; title: string }) {
  return (
    <div className="flex min-h-105 items-center justify-center bg-slate-950 p-4">
      <MediaController className="block w-full max-w-5xl overflow-hidden rounded-lg bg-black shadow-2xl">
        <video
          className="block max-h-[72vh] w-full bg-black"
          slot="media"
          src={src}
          title={title}
          preload="metadata"
        />
        <MediaLoadingIndicator slot="centered-chrome" className="text-white" />
        <MediaControlBar className="bg-black/85 px-2 py-1 text-white">
          <MediaPlayButton />
          <MediaSeekBackwardButton seekOffset={10} />
          <MediaSeekForwardButton seekOffset={10} />
          <MediaTimeDisplay />
          <MediaTimeRange className="min-w-0 flex-1" />
          <MediaDurationDisplay />
          <MediaMuteButton />
          <MediaVolumeRange className="w-24" />
          <MediaPipButton />
          <MediaFullscreenButton />
        </MediaControlBar>
      </MediaController>
    </div>
  );
}


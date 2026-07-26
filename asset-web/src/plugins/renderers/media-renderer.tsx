import {
  MediaControlBar,
  MediaController,
  MediaMuteButton,
  MediaPlayButton,
  MediaTimeDisplay,
  MediaTimeRange,
  MediaVolumeRange,
} from "media-chrome/react";
import type { PluginViewRendererProps } from "@/kernel/plugin-kernel";

export default function MediaRenderer({ view, gateway }: PluginViewRendererProps) {
  if (view.view !== "media") return null;
  const source =
    view.encoding === "base64"
      ? `data:${view.mime_type};base64,${view.data}`
      : gateway.assetUrl(view.data);
  if (!source) return <InvalidMediaUrl />;
  return <MediaContent source={source} mimeType={view.mime_type} title={view.title} />;
}

function InvalidMediaUrl() {
  return (
    <p className="m-4 rounded-xl bg-red-50 p-4 text-sm text-red-700">
      The plugin returned an invalid or external media URL.
    </p>
  );
}

function MediaContent({
  source,
  mimeType,
  title,
}: {
  source: string;
  mimeType: string;
  title?: string | undefined;
}) {
  if (mimeType.startsWith("image/"))
    return (
      <img
        className="mx-auto max-h-[70vh] max-w-full object-contain"
        src={source}
        alt={title ?? "Plugin output"}
      />
    );
  if (mimeType.startsWith("audio/")) {
    return (
      <MediaController className="w-full bg-slate-950">
        {/* biome-ignore lint/a11y/useMediaCaption: the plugin media ABI does not expose transcript tracks */}
        <audio slot="media" src={source} preload="metadata" />
        <MediaControlBar>
          <MediaPlayButton />
          <MediaTimeDisplay />
          <MediaTimeRange />
          <MediaMuteButton />
          <MediaVolumeRange />
        </MediaControlBar>
      </MediaController>
    );
  }
  return (
    <p className="m-4 rounded-xl bg-slate-100 p-4 text-sm text-slate-600">
      No host renderer is available for media type <code>{mimeType}</code>.
    </p>
  );
}

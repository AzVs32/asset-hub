import {
  MediaControlBar,
  MediaController,
  MediaFullscreenButton,
  MediaMuteButton,
  MediaPlayButton,
  MediaTimeDisplay,
  MediaTimeRange,
  MediaVolumeRange,
} from "media-chrome/react";
import type { PluginViewRendererProps } from "@/kernel/plugin-kernel";
import { Button } from "@/shared/ui/button";

export default function MediaRenderer({ view, gateway }: PluginViewRendererProps) {
  if (view.view === "media") {
    const source =
      view.encoding === "base64"
        ? `data:${view.mime_type};base64,${view.data}`
        : gateway.assetUrl(view.data);
    if (!source) return <InvalidMediaUrl />;
    return <MediaContent source={source} mimeType={view.mime_type} title={view.title} />;
  }
  if (view.view !== "binary_url") return null;
  const source = gateway.assetUrl(view.url);
  if (!source) return <InvalidMediaUrl />;
  if (
    view.mime_type?.startsWith("image/") ||
    view.mime_type?.startsWith("video/") ||
    view.mime_type?.startsWith("audio/")
  ) {
    return <MediaContent source={source} mimeType={view.mime_type} title={view.filename} />;
  }
  if (view.mime_type === "application/pdf") {
    return (
      <iframe
        className="h-[65vh] min-h-96 w-full border-0"
        src={source}
        title={view.filename ?? "PDF"}
      />
    );
  }
  return (
    <div className="grid min-h-48 place-items-center bg-slate-50 p-6">
      <Button onClick={() => window.open(source, "_blank", "noopener,noreferrer")}>
        Open file
      </Button>
    </div>
  );
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
  if (mimeType.startsWith("video/")) {
    return (
      <MediaController className="aspect-video w-full bg-black">
        {/* biome-ignore lint/a11y/useMediaCaption: the plugin media ABI does not expose caption tracks */}
        <video
          slot="media"
          className="h-full w-full object-contain"
          src={source}
          playsInline
          preload="metadata"
        />
        <MediaControlBar>
          <MediaPlayButton />
          <MediaTimeDisplay />
          <MediaTimeRange />
          <MediaMuteButton />
          <MediaVolumeRange />
          <MediaFullscreenButton />
        </MediaControlBar>
      </MediaController>
    );
  }
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
  return <pre className="p-5 text-xs">{JSON.stringify({ source, mimeType }, null, 2)}</pre>;
}

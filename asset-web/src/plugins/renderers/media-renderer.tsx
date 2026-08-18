import { Alert } from "@mui/material";
import {
  MediaControlBar,
  MediaController,
  MediaMuteButton,
  MediaPlayButton,
  MediaTimeDisplay,
  MediaTimeRange,
  MediaVolumeRange,
} from "media-chrome/react";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { PluginView } from "@/domain/plugin";

export default function MediaRenderer({
  view,
  gateway,
}: {
  view: Extract<PluginView, { view: "media" }>;
  gateway: AssetGateway;
}) {
  const source =
    view.encoding === "base64"
      ? `data:${view.mime_type};base64,${view.data}`
      : gateway.assetUrl(view.data);
  if (!source) return <InvalidMediaUrl />;
  return <MediaContent source={source} mimeType={view.mime_type} title={view.title} />;
}

function InvalidMediaUrl() {
  return (
    <Alert severity="error" sx={{ m: 2 }}>
      The plugin returned an invalid or external media URL.
    </Alert>
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
        style={{
          display: "block",
          margin: "0 auto",
          maxHeight: "70vh",
          maxWidth: "100%",
          objectFit: "contain",
        }}
        src={source}
        alt={title ?? "Plugin output"}
      />
    );
  if (mimeType.startsWith("audio/")) {
    return (
      <MediaController style={{ width: "100%" }}>
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
  if (mimeType.startsWith("video/")) {
    return (
      <MediaController style={{ width: "100%" }}>
        {/* biome-ignore lint/a11y/useMediaCaption: the plugin media ABI does not expose transcript tracks */}
        <video
          slot="media"
          style={{ maxHeight: "70vh", width: "100%" }}
          src={source}
          preload="metadata"
        />
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
    <Alert severity="info" sx={{ m: 2 }}>
      No host renderer is available for media type <code>{mimeType}</code>.
    </Alert>
  );
}

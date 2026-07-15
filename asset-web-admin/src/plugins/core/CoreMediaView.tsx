import { apiBase } from "../../api";
import { primaryButtonClass } from "../../components/ui";
import type { PluginView } from "../../types";
import { CoreVideoView } from "./CoreVideoView";

type MediaView = Extract<PluginView, { view: "media" }>;
type BinaryUrlView = Extract<PluginView, { view: "binary_url" }>;

export function CoreMediaView({ view, title }: { view: MediaView; title: string }) {
  const src = view.encoding === "base64"
    ? `data:${view.mime_type};base64,${view.data}`
    : assetHubUrl(view.data);
  if (!src) return <InvalidAssetUrl />;

  if (view.mime_type.startsWith("image/")) {
    return (
      <div className="flex min-h-105 items-center justify-center bg-slate-50 p-6">
        <img className="max-h-[72vh] max-w-full rounded-lg object-contain" src={src} alt={view.title || title} />
      </div>
    );
  }

  if (view.mime_type.startsWith("video/")) {
    return <CoreVideoView src={src} title={view.title || title} />;
  }

  if (view.mime_type === "application/pdf") {
    return <iframe className="block h-[72vh] max-h-180 w-full border-0 bg-white" title={view.title || title} src={src} />;
  }

  return (
    <div className="min-h-80 overflow-auto bg-slate-50 p-5">
      <a className={primaryButtonClass} href={src} download={view.title || title}>
        Download
      </a>
    </div>
  );
}

export function CoreBinaryUrlView({ view }: { view: BinaryUrlView }) {
  const src = assetHubUrl(view.url);
  if (!src) return <InvalidAssetUrl />;

  return (
    <div className="min-h-80 overflow-auto bg-slate-50 p-5">
      <a className={primaryButtonClass} href={src} target="_blank" rel="noreferrer">
        Open
      </a>
    </div>
  );
}

function assetHubUrl(value: string): string {
  if (!value.startsWith("/") || value.startsWith("//")) return "";
  return `${apiBase}${value}`;
}

function InvalidAssetUrl() {
  return <div className="min-h-40 bg-slate-50 p-5 text-sm text-red-700">Invalid or external plugin URL</div>;
}

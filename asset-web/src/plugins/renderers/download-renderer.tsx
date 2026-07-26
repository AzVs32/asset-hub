import type { PluginViewRendererProps } from "@/kernel/plugin-kernel";

export default function DownloadRenderer({ view, gateway }: PluginViewRendererProps) {
  if (view.view !== "download") return null;
  const source = gateway.assetUrl(view.url);
  if (!source) {
    return (
      <p className="m-4 rounded-xl bg-red-50 p-4 text-sm text-red-700">
        The plugin returned an invalid or external download URL.
      </p>
    );
  }
  return (
    <div className="grid min-h-48 place-items-center bg-slate-50 p-6">
      <a
        className="inline-flex min-h-10 items-center justify-center rounded-lg bg-blue-600 px-4 text-sm font-semibold text-white transition hover:bg-blue-700 focus-visible:outline-none focus-visible:ring-4 focus-visible:ring-blue-200"
        href={source}
        download={view.filename ?? true}
      >
        Download file
      </a>
    </div>
  );
}

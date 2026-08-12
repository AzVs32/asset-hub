import ReactMarkdown from "react-markdown";
import { useGateway } from "@/application/ports/gateway-context";
import type { DirectoryActionOutput, PluginView } from "@/domain/plugin";
import type { Directory, DirectoryAction } from "@/domain/resource";
import { Dialog } from "@/shared/ui/dialog";
import { DirectoryPluginFrame } from "./directory-plugin-frame";

export interface DirectoryActionResult {
  directory: Directory;
  action: DirectoryAction;
  output: DirectoryActionOutput;
}

export function DirectoryActionDialog({
  result,
  onClose,
  onDirectoryChanged,
  onNavigate,
}: {
  result: DirectoryActionResult | null;
  onClose: () => void;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
}) {
  return (
    <Dialog
      open={Boolean(result)}
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
      title={result?.action.label ?? "Directory action"}
      description={result ? `${result.directory.path || "/"} · ${result.action.id}` : undefined}
      className="max-w-5xl"
    >
      {result ? (
        <div>
          {result.output.diagnostics.length ? (
            <div className="m-4 rounded-2xl border border-amber-200 bg-amber-50 p-4 text-sm text-amber-950 shadow-sm">
              {result.output.diagnostics.map((item) => (
                <p key={`${item.code}:${item.message}`}>{item.message}</p>
              ))}
            </div>
          ) : null}
          {result.output.view ? (
            <DirectoryView
              result={result}
              view={result.output.view}
              onDirectoryChanged={onDirectoryChanged}
              onNavigate={onNavigate}
            />
          ) : null}
        </div>
      ) : null}
    </Dialog>
  );
}

function DirectoryView({
  result,
  view,
  onDirectoryChanged,
  onNavigate,
}: {
  result: DirectoryActionResult;
  view: PluginView;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
}) {
  const gateway = useGateway();
  if (view.view === "text")
    return <pre className="whitespace-pre-wrap p-5 text-sm">{view.text}</pre>;
  if (view.view === "markdown")
    return (
      <article className="plugin-prose p-5">
        <ReactMarkdown>{view.markdown}</ReactMarkdown>
      </article>
    );
  if (view.view === "json")
    return (
      <pre className="max-h-[65vh] overflow-auto bg-slate-950 p-5 text-xs leading-6 text-slate-100">
        {JSON.stringify(view.data, null, 2)}
      </pre>
    );
  if (view.view === "html")
    return (
      <iframe
        className="block h-[65vh] min-h-80 w-full border-0"
        sandbox=""
        title={view.title ?? "Directory action output"}
        srcDoc={view.html}
      />
    );
  if (view.view === "plugin_frame") {
    return (
      <DirectoryPluginFrame
        directory={result.directory}
        output={result.output}
        view={view}
        gateway={gateway}
        onDirectoryChanged={onDirectoryChanged}
        onNavigate={onNavigate}
        className="block h-[70vh] min-h-96 w-full border-0"
      />
    );
  }
  if (view.view === "download") {
    const source = gateway.assetUrl(view.url);
    return source ? (
      <a
        className="m-5 inline-flex text-sm font-medium text-blue-700 underline"
        href={source}
        download={view.filename}
      >
        Download {view.filename ?? "file"}
      </a>
    ) : null;
  }
  const source =
    view.encoding === "url"
      ? gateway.assetUrl(view.data)
      : `data:${view.mime_type};base64,${view.data}`;
  if (!source) return null;
  return view.mime_type.startsWith("image/") ? (
    <img
      className="max-h-[70vh] w-full object-contain p-5"
      src={source}
      alt={view.title ?? "Directory action media"}
    />
  ) : (
    // biome-ignore lint/a11y/useMediaCaption: the plugin media ABI does not expose transcript tracks
    <video className="max-h-[70vh] w-full" src={source} controls />
  );
}

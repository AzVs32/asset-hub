import React from "react";
import type { PluginActionOutput, PluginView } from "@/domain/plugin";
import type { ResourceAction } from "@/domain/resource";
import type { PluginKernel, PluginViewRendererProps } from "@/kernel/plugin-kernel";
import { parseExecuteActionMessage, pluginFrameProtocolVersion } from "../frame-protocol";

const MarkdownRenderer = React.lazy(() => import("./markdown-renderer"));
const MediaRenderer = React.lazy(() => import("./media-renderer"));
const FormRenderer = React.lazy(() => import("./form-renderer"));

export function registerDefaultViewRenderers(kernel: PluginKernel): void {
  for (const kind of [
    "text",
    "markdown",
    "html",
    "plugin_frame",
    "json",
    "media",
    "binary_url",
    "table",
    "form",
  ] as const) {
    kernel.registerView(kind, DefaultViewRenderer);
  }
}

function DefaultViewRenderer(props: PluginViewRendererProps) {
  const { view } = props;
  if (view.view === "text") return <TextView text={view.text} />;
  if (view.view === "markdown")
    return (
      <LazyView>
        <MarkdownRenderer {...props} />
      </LazyView>
    );
  if (view.view === "html") return <HtmlView view={view} />;
  if (view.view === "plugin_frame") return <PluginFrameView {...props} view={view} />;
  if (view.view === "media" || view.view === "binary_url")
    return (
      <LazyView>
        <MediaRenderer {...props} />
      </LazyView>
    );
  if (view.view === "table") return <TableView view={view} />;
  if (view.view === "form")
    return (
      <LazyView>
        <FormRenderer {...props} />
      </LazyView>
    );
  return <JsonView value={view.data} />;
}

function TextView({ text }: { text: string }) {
  return <article className="plugin-prose whitespace-pre-wrap">{text}</article>;
}

function HtmlView({ view }: { view: Extract<PluginView, { view: "html" }> }) {
  return (
    <iframe
      className="block h-[65vh] min-h-80 w-full border-0 bg-white"
      sandbox=""
      title={view.title ?? "Plugin HTML output"}
      srcDoc={htmlWithoutNetwork(view.html)}
    />
  );
}

function JsonView({ value }: { value: unknown }) {
  return (
    <pre className="max-h-[65vh] overflow-auto bg-slate-950 p-5 font-mono text-xs leading-6 text-slate-100">
      {JSON.stringify(value, null, 2)}
    </pre>
  );
}

function TableView({ view }: { view: Extract<PluginView, { view: "table" }> }) {
  return (
    <div className="max-h-[65vh] overflow-auto">
      <table className="w-full border-collapse text-left text-sm">
        <thead className="sticky top-0 bg-slate-100 text-xs uppercase tracking-wide text-slate-600">
          <tr>
            {view.columns.map((column) => (
              <th className="border-b border-slate-200 px-4 py-3" key={column.key}>
                {column.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {view.rows.map((row) => (
            <tr
              className="even:bg-slate-50"
              key={rowIdentity(
                row,
                view.columns.map((column) => column.key),
              )}
            >
              {view.columns.map((column) => (
                <td className="border-b border-slate-100 px-4 py-3 align-top" key={column.key}>
                  {tableCell(row, column.key)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function PluginFrameView({
  view,
  resource,
  gateway,
  onResourceChanged,
}: PluginViewRendererProps & { view: Extract<PluginView, { view: "plugin_frame" }> }) {
  const ref = React.useRef<HTMLIFrameElement>(null);
  const source = pluginFrameUrl(view.url, gateway.assetUrl.bind(gateway));

  React.useEffect(() => {
    async function receive(event: MessageEvent) {
      if (event.source !== ref.current?.contentWindow) return;
      const message = parseExecuteActionMessage(event.data);
      if (!message || !source || message.resourceId !== resource.id) return;
      const action = resource.actions.find((candidate) => candidate.id === message.action);
      if (!action) {
        postResult(
          ref.current,
          message.requestId,
          null,
          `Action ${message.action} is not available.`,
        );
        return;
      }
      try {
        const result = await gateway.executeAction(resource, action.id, message.input ?? {});
        postResult(ref.current, message.requestId, result, null);
        if (action.access === "read_write") await onResourceChanged?.();
      } catch (cause) {
        postResult(
          ref.current,
          message.requestId,
          null,
          cause instanceof Error ? cause.message : "Action failed",
        );
      }
    }
    window.addEventListener("message", receive);
    return () => window.removeEventListener("message", receive);
  }, [gateway, onResourceChanged, resource, source]);

  if (!source) return <PluginError message="The plugin returned an invalid frame URL." />;
  return (
    <iframe
      ref={ref}
      className="block h-[70vh] min-h-96 w-full border-0 bg-white"
      sandbox="allow-scripts"
      src={source}
      title={view.title ?? "Plugin view"}
    />
  );
}

function PluginError({ message }: { message: string }) {
  return (
    <p className="m-4 rounded-xl border border-red-200 bg-red-50 p-4 text-sm text-red-700">
      {message}
    </p>
  );
}

function pluginFrameUrl(value: string, resolveUrl: (url: string) => string | null): string | null {
  if (!/^\/plugins\/[a-z0-9._-]+\/(?!.*(?:^|\/)\.\.(?:\/|$))/.test(value)) return null;
  return resolveUrl(value);
}

function postResult(
  frame: HTMLIFrameElement | null,
  requestId: string,
  data: unknown,
  error: string | null,
) {
  frame?.contentWindow?.postMessage(
    {
      type: "asset-hub:execute-resource-action-result",
      version: pluginFrameProtocolVersion,
      request_id: requestId,
      ok: error === null,
      data,
      error,
    },
    "*",
  );
}

function htmlWithoutNetwork(html: string): string {
  const policy =
    "default-src 'none'; img-src data:; media-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
  return `<meta http-equiv="Content-Security-Policy" content="${policy}">${html}`;
}

function tableCell(row: unknown, key: string): string {
  const value =
    row && typeof row === "object" && !Array.isArray(row)
      ? (row as Record<string, unknown>)[key]
      : undefined;
  return typeof value === "string" ? value : JSON.stringify(value ?? "");
}

function rowIdentity(row: unknown, columns: string[]): string {
  if (row && typeof row === "object" && !Array.isArray(row)) {
    const id = (row as Record<string, unknown>).id;
    if (typeof id === "string" || typeof id === "number") return String(id);
    return columns.map((column) => tableCell(row, column)).join("\u001f");
  }
  return JSON.stringify(row);
}

export function actionTitle(action: ResourceAction, output: PluginActionOutput): string {
  const view = output.view;
  if ((view.view === "html" || view.view === "plugin_frame" || view.view === "media") && view.title)
    return view.title;
  if (view.view === "binary_url" && view.filename) return view.filename;
  return action.label;
}

function LazyView({ children }: { children: React.ReactNode }) {
  return (
    <React.Suspense
      fallback={
        <div className="grid min-h-40 place-items-center text-sm text-slate-500">
          Loading plugin renderer…
        </div>
      }
    >
      {children}
    </React.Suspense>
  );
}

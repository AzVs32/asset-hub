import React from "react";
import type { PluginView, ResourceActionOutput } from "@/domain/plugin";
import type { ResourceAction } from "@/domain/resource";
import type { PluginKernel, PluginViewRendererProps } from "@/kernel/plugin-kernel";
import { parsePluginFrameRequest } from "../frame-protocol";

const MarkdownRenderer = React.lazy(() => import("./markdown-renderer"));
const MediaRenderer = React.lazy(() => import("./media-renderer"));
const DownloadRenderer = React.lazy(() => import("./download-renderer"));

export function registerDefaultViewRenderers(kernel: PluginKernel): void {
  for (const kind of [
    "text",
    "markdown",
    "html",
    "plugin_frame",
    "json",
    "media",
    "download",
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
  if (view.view === "media")
    return (
      <LazyView>
        <MediaRenderer {...props} />
      </LazyView>
    );
  if (view.view === "download")
    return (
      <LazyView>
        <DownloadRenderer {...props} />
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

function PluginFrameView({
  view,
  output,
  resource,
  gateway,
  onResourceChanged,
}: PluginViewRendererProps & { view: Extract<PluginView, { view: "plugin_frame" }> }) {
  const ref = React.useRef<HTMLIFrameElement>(null);
  const [frameResource, setFrameResource] = React.useState(resource);
  const source = pluginFrameUrl(view.url, gateway.assetUrl.bind(gateway));

  React.useEffect(() => {
    setFrameResource((current) =>
      current.id !== resource.id || resource.revision > current.revision ? resource : current,
    );
  }, [resource]);

  React.useEffect(() => {
    async function receive(event: MessageEvent) {
      if (event.source !== ref.current?.contentWindow) return;
      const message = parsePluginFrameRequest(event.data, view.plugin_api);
      if (!message || !source || message.resourceId !== frameResource.id) return;
      if (message.type === "asset-hub:replace-resource-text") {
        const editAction = frameResource.actions.find(
          (candidate) =>
            output.resourceId === frameResource.id &&
            candidate.id === output.action &&
            candidate.provides === "text_edit" &&
            candidate.access === "write",
        );
        if (!editAction) {
          postResult(
            ref.current,
            message.type,
            view.plugin_api,
            message.requestId,
            null,
            "Text editing is not available from this frame.",
          );
          return;
        }
        try {
          const updated = await gateway.replaceResourceText(frameResource, message.text);
          setFrameResource(updated);
          postResult(ref.current, message.type, view.plugin_api, message.requestId, null, null);
          await onResourceChanged?.();
        } catch (cause) {
          postResult(
            ref.current,
            message.type,
            view.plugin_api,
            message.requestId,
            null,
            cause instanceof Error ? cause.message : "Content replacement failed",
          );
        }
        return;
      }
      const action = frameResource.actions.find((candidate) => candidate.id === message.action);
      if (!action) {
        postResult(
          ref.current,
          message.type,
          view.plugin_api,
          message.requestId,
          null,
          `Action ${message.action} is not available.`,
        );
        return;
      }
      try {
        const result = await gateway.executeAction(frameResource, action.id, message.input ?? {});
        postResult(ref.current, message.type, view.plugin_api, message.requestId, result, null);
        if (action.access === "write") await onResourceChanged?.();
      } catch (cause) {
        postResult(
          ref.current,
          message.type,
          view.plugin_api,
          message.requestId,
          null,
          cause instanceof Error ? cause.message : "Action failed",
        );
      }
    }
    window.addEventListener("message", receive);
    return () => window.removeEventListener("message", receive);
  }, [
    frameResource,
    gateway,
    onResourceChanged,
    output.action,
    output.resourceId,
    source,
    view.plugin_api,
  ]);

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
  requestType: "asset-hub:execute-resource-action" | "asset-hub:replace-resource-text",
  pluginApi: string,
  requestId: string,
  data: unknown,
  error: string | null,
) {
  frame?.contentWindow?.postMessage(
    {
      type: `${requestType}-result`,
      plugin_api: pluginApi,
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

export function actionTitle(action: ResourceAction, output: ResourceActionOutput): string {
  const view = output.view;
  if ((view.view === "html" || view.view === "plugin_frame" || view.view === "media") && view.title)
    return view.title;
  if (view.view === "download" && view.filename) return view.filename;
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

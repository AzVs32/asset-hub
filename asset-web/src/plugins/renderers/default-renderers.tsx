import { connect, WindowMessenger } from "penpal";
import React from "react";
import type { PluginView, ResourceActionOutput } from "@/domain/plugin";
import type { ResourceAction } from "@/domain/resource";
import type { PluginKernel, PluginViewRendererProps } from "@/kernel/plugin-kernel";
import {
  createPluginFrameHostBridge,
  pluginFrameApiVersion,
  pluginFrameChannel,
} from "../frame-host";

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
  const source = pluginFrameUrl(view.url, gateway.assetUrl.bind(gateway));
  const onResourceChangedRef = React.useRef(onResourceChanged);
  const resourceRef = React.useRef(resource);
  onResourceChangedRef.current = onResourceChanged;
  resourceRef.current = resource;
  const selectedResourceId = resource.id;
  const bridge = React.useMemo(() => {
    const initialResource = resourceRef.current;
    if (initialResource.id !== selectedResourceId) {
      throw new Error("The plugin frame Resource changed during connection setup.");
    }
    return createPluginFrameHostBridge({
      resource: initialResource,
      frameResourceId: output.resourceId,
      frameActionId: output.action,
      gateway,
      onResourceChanged: () => onResourceChangedRef.current?.(),
    });
  }, [gateway, output.action, output.resourceId, selectedResourceId]);

  React.useEffect(() => {
    bridge.updateResource(resource);
  }, [bridge, resource]);

  React.useEffect(() => {
    const remoteWindow = ref.current?.contentWindow;
    if (!source || !remoteWindow || view.plugin_api !== pluginFrameApiVersion) return;
    const messenger = new WindowMessenger({
      remoteWindow,
      // The sandbox intentionally creates an opaque origin. Penpal still binds this exact Window.
      allowedOrigins: ["*"],
    });
    const connection = connect({
      messenger,
      channel: pluginFrameChannel,
      methods: bridge.methods,
    });
    return () => connection.destroy();
  }, [bridge, source, view.plugin_api]);

  if (!source) return <PluginError message="The plugin returned an invalid frame URL." />;
  if (view.plugin_api !== pluginFrameApiVersion)
    return <PluginError message={`Unsupported Plugin Frame API: ${view.plugin_api}`} />;
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

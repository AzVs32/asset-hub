import { Alert, Box, CircularProgress } from "@mui/material";
import { connect } from "penpal";
import React from "react";
import {
  PLUGIN_API_VERSION,
  type PluginView,
  pluginViewKinds,
  RESOURCE_FRAME_CHANNEL,
  type ResourceActionOutput,
} from "@/domain/plugin";
import type { ResourceAction } from "@/domain/resource";
import type { PluginKernel, PluginViewRendererProps } from "@/kernel/plugin-kernel";
import { createPluginFrameMessenger, pluginFrameUrl } from "../frame-boundary";
import { createPluginFrameHostBridge } from "../frame-host";

const MarkdownRenderer = React.lazy(() => import("./markdown-renderer"));
const MediaRenderer = React.lazy(() => import("./media-renderer"));
const DownloadRenderer = React.lazy(() => import("./download-renderer"));

export function registerDefaultViewRenderers(kernel: PluginKernel): void {
  for (const kind of pluginViewKinds) {
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
  return (
    <Box component="article" className="plugin-prose" sx={{ whiteSpace: "pre-wrap" }}>
      {text}
    </Box>
  );
}

function HtmlView({ view }: { view: Extract<PluginView, { view: "html" }> }) {
  return (
    <Box
      component="iframe"
      sandbox=""
      title={view.title ?? "Plugin HTML output"}
      srcDoc={htmlWithoutNetwork(view.html)}
      sx={{
        display: "block",
        height: "65vh",
        minHeight: "20rem",
        width: "100%",
        border: 0,
      }}
    />
  );
}

function JsonView({ value }: { value: unknown }) {
  return (
    <Box
      component="pre"
      sx={{
        maxHeight: "65vh",
        overflow: "auto",
        p: 2,
        fontFamily: "monospace",
        fontSize: "0.875rem",
        whiteSpace: "pre-wrap",
        m: 0,
      }}
    >
      {JSON.stringify(value, null, 2)}
    </Box>
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
      confirmAction: (message) => window.confirm(message),
    });
  }, [gateway, output.action, output.resourceId, selectedResourceId]);

  React.useEffect(() => {
    bridge.updateResource(resource);
  }, [bridge, resource]);

  React.useEffect(() => {
    const remoteWindow = ref.current?.contentWindow;
    if (!source || !remoteWindow || view.plugin_api !== PLUGIN_API_VERSION) return;
    const connection = connect({
      messenger: createPluginFrameMessenger(remoteWindow),
      channel: RESOURCE_FRAME_CHANNEL,
      methods: bridge.methods,
    });
    return () => connection.destroy();
  }, [bridge, source, view.plugin_api]);

  if (!source) return <PluginError message="The plugin returned an invalid frame URL." />;
  if (view.plugin_api !== PLUGIN_API_VERSION)
    return <PluginError message={`Unsupported Plugin Frame API: ${view.plugin_api}`} />;
  return (
    <iframe
      ref={ref}
      style={{
        display: "block",
        height: "70vh",
        minHeight: "24rem",
        width: "100%",
        border: 0,
      }}
      sandbox="allow-scripts"
      src={source}
      title={view.title ?? "Plugin view"}
    />
  );
}

function PluginError({ message }: { message: string }) {
  return (
    <Alert severity="error" sx={{ m: 2 }}>
      {message}
    </Alert>
  );
}

function htmlWithoutNetwork(html: string): string {
  const policy =
    "default-src 'none'; img-src data:; media-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
  return `<meta http-equiv="Content-Security-Policy" content="${policy}">${html}`;
}

export function actionTitle(action: ResourceAction, output: ResourceActionOutput): string {
  const view = output.view;
  if (!view) return action.label;
  if ((view.view === "html" || view.view === "plugin_frame" || view.view === "media") && view.title)
    return view.title;
  if (view.view === "download" && view.filename) return view.filename;
  return action.label;
}

function LazyView({ children }: { children: React.ReactNode }) {
  return (
    <React.Suspense
      fallback={
        <Box sx={{ display: "grid", minHeight: "10rem", placeItems: "center" }}>
          <CircularProgress size={20} />
        </Box>
      }
    >
      {children}
    </React.Suspense>
  );
}

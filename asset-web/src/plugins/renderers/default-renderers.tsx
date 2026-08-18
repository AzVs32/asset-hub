import { Alert } from "@mui/material";
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
import { GenericPluginViewRenderer } from "./generic-plugin-view";

export function registerDefaultViewRenderers(kernel: PluginKernel): void {
  for (const kind of pluginViewKinds) {
    kernel.registerView(kind, DefaultViewRenderer);
  }
}

function DefaultViewRenderer(props: PluginViewRendererProps) {
  const { view } = props;
  if (view.view === "plugin_frame") return <PluginFrameView {...props} view={view} />;
  return <GenericPluginViewRenderer view={view} gateway={props.gateway} />;
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

export function actionTitle(action: ResourceAction, output: ResourceActionOutput): string {
  const view = output.view;
  if (!view) return action.label;
  if ((view.view === "html" || view.view === "plugin_frame" || view.view === "media") && view.title)
    return view.title;
  if (view.view === "download" && view.filename) return view.filename;
  return action.label;
}

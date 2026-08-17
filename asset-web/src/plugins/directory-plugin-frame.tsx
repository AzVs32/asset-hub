import { connect } from "penpal";
import React from "react";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import {
  DIRECTORY_FRAME_CHANNEL,
  type DirectoryActionOutput,
  PLUGIN_API_VERSION,
  type PluginView,
} from "@/domain/plugin";
import type { Directory, Resource, ResourceAction } from "@/domain/resource";
import { createDirectoryPluginFrameHostBridge } from "./directory-frame-host";
import { createPluginFrameMessenger, pluginFrameUrl } from "./frame-boundary";

export function DirectoryPluginFrame({
  directory,
  output,
  view,
  gateway,
  onDirectoryChanged,
  onNavigate,
  onEditResource,
  instanceVersion = 0,
  className = "block min-h-96 w-full flex-1 border-0 bg-white",
}: {
  directory: Directory;
  output: DirectoryActionOutput;
  view: Extract<PluginView, { view: "plugin_frame" }>;
  gateway: AssetGateway;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
  onEditResource?:
    | ((resource: Resource, action: ResourceAction) => void | Promise<void>)
    | undefined;
  instanceVersion?: number;
  className?: string;
}) {
  const ref = React.useRef<HTMLIFrameElement>(null);
  const source = pluginFrameUrl(view.url, gateway.assetUrl.bind(gateway));
  const directoryRef = React.useRef(directory);
  const onDirectoryChangedRef = React.useRef(onDirectoryChanged);
  const onNavigateRef = React.useRef(onNavigate);
  const onEditResourceRef = React.useRef(onEditResource);
  directoryRef.current = directory;
  onDirectoryChangedRef.current = onDirectoryChanged;
  onNavigateRef.current = onNavigate;
  onEditResourceRef.current = onEditResource;
  const directoryId = directory.id;
  const bridge = React.useMemo(() => {
    const initialDirectory = directoryRef.current;
    if (initialDirectory.id !== directoryId) {
      throw new Error("The Directory changed during plugin frame connection setup.");
    }
    return createDirectoryPluginFrameHostBridge({
      directory: initialDirectory,
      frameDirectoryId: output.directoryId,
      gateway,
      onDirectoryChanged: () => onDirectoryChangedRef.current?.(),
      onNavigate: (path) => onNavigateRef.current?.(path),
      onEditResource: async (resource, action) => {
        const callback = onEditResourceRef.current;
        if (!callback) {
          throw new Error("Resource editing is not available from this Directory frame.");
        }
        await callback(resource, action);
      },
      confirmAction: (message) => window.confirm(message),
    });
  }, [directoryId, gateway, output.directoryId]);

  React.useEffect(() => {
    bridge.updateDirectory(directory);
  }, [bridge, directory]);

  React.useEffect(() => {
    const frame = ref.current;
    const remoteWindow = frame?.contentWindow;
    if (frame?.dataset.instanceVersion !== String(instanceVersion)) return;
    if (!source || !remoteWindow || view.plugin_api !== PLUGIN_API_VERSION) return;
    const connection = connect({
      messenger: createPluginFrameMessenger(remoteWindow),
      channel: DIRECTORY_FRAME_CHANNEL,
      methods: bridge.methods,
    });
    return () => connection.destroy();
  }, [bridge, instanceVersion, source, view.plugin_api]);

  if (!source) return <FrameError message="The plugin returned an invalid frame URL." />;
  if (view.plugin_api !== PLUGIN_API_VERSION) {
    return <FrameError message={`Unsupported Plugin Frame API: ${view.plugin_api}`} />;
  }
  return (
    <iframe
      key={`${directoryId}:${instanceVersion}`}
      ref={ref}
      data-instance-version={instanceVersion}
      className={className}
      sandbox="allow-scripts"
      src={source}
      title={view.title ?? "Directory plugin workspace"}
    />
  );
}

function FrameError({ message }: { message: string }) {
  return (
    <p className="m-4 rounded-xl border border-red-200 bg-red-50 p-4 text-sm text-red-700">
      {message}
    </p>
  );
}

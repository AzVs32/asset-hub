import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { WorkspaceResource } from "@/application/workspace/workspace-scope";
import type { PluginActionOutput } from "@/domain/plugin";
import { usePluginKernel } from "./plugin-kernel";

export function PluginViewHost({
  output,
  resource,
  gateway,
  onResourceChanged,
}: {
  output: PluginActionOutput;
  resource: WorkspaceResource;
  gateway: AssetGateway;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}) {
  const kernel = usePluginKernel();
  const Renderer = kernel.viewRenderer(output.view.view);
  if (!Renderer) {
    return (
      <div className="plugin-unsupported">
        No host renderer is registered for <code>{output.view.view}</code>.
      </div>
    );
  }
  return (
    <Renderer
      view={output.view}
      output={output}
      resource={resource}
      gateway={gateway}
      {...(onResourceChanged ? { onResourceChanged } : {})}
    />
  );
}

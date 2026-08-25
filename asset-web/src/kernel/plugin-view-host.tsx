import { Alert } from "@mui/material";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import { type ResourceChangedHandler, usePluginKernel } from "./plugin-kernel";

export function PluginViewHost({
  output,
  resource,
  gateway,
  onResourceChanged,
}: {
  output: ResourceActionOutput;
  resource: Resource;
  gateway: AssetGateway;
  onResourceChanged?: ResourceChangedHandler | undefined;
}) {
  const kernel = usePluginKernel();
  if (!output.view) return null;
  const Renderer = kernel.viewRenderer(output.view.type);
  if (!Renderer) {
    return (
      <Alert severity="warning" sx={{ m: 2 }}>
        No host renderer is registered for <code>{output.view.type}</code>.
      </Alert>
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

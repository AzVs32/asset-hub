import { Alert } from "@mui/material";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import type { PluginHostGateway } from "@/shared/api/gateways";
import { type ResourceChangedHandler, usePluginKernel } from "./plugin-kernel";

export function ResourcePluginViewHost({
  output,
  resource,
  gateway,
  onResourceChanged,
}: {
  output: ResourceActionOutput;
  resource: Resource;
  gateway: PluginHostGateway;
  onResourceChanged?: ResourceChangedHandler | undefined;
}) {
  const kernel = usePluginKernel();
  if (!output.view) return null;
  const Renderer = kernel.resourceViewRenderer(output.view.type);
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

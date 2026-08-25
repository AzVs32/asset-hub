import { Box } from "@mui/material";
import { useGateway } from "@/application/ports/gateway-context";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import type { ResourceChangedHandler } from "@/kernel/plugin-kernel";
import { ResourcePluginViewHost } from "@/kernel/resource-plugin-view-host";
import { PluginDiagnostics } from "./plugin-diagnostics";

export function ResourcePluginOutput({
  output,
  resource,
  onResourceChanged,
}: {
  output: ResourceActionOutput;
  resource: Resource;
  onResourceChanged?: ResourceChangedHandler | undefined;
}) {
  const gateway = useGateway();
  return (
    <Box>
      <PluginDiagnostics diagnostics={output.diagnostics} sx={{ m: 2 }} />
      <ResourcePluginViewHost
        output={output}
        resource={resource}
        gateway={gateway}
        onResourceChanged={onResourceChanged}
      />
    </Box>
  );
}

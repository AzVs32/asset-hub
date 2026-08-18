import { Box } from "@mui/material";
import { useGateway } from "@/application/ports/gateway-context";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import { PluginViewHost } from "@/kernel/plugin-view-host";
import { PluginDiagnostics } from "./plugin-diagnostics";

export function PluginOutput({
  output,
  resource,
  onResourceChanged,
}: {
  output: ResourceActionOutput;
  resource: Resource;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}) {
  const gateway = useGateway();
  return (
    <Box>
      <PluginDiagnostics diagnostics={output.diagnostics} sx={{ m: 2 }} />
      <PluginViewHost
        output={output}
        resource={resource}
        gateway={gateway}
        onResourceChanged={onResourceChanged}
      />
    </Box>
  );
}

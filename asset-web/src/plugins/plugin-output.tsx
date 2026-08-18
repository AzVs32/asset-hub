import { Alert, Box, Stack } from "@mui/material";
import { useGateway } from "@/application/ports/gateway-context";
import type { ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import { PluginViewHost } from "@/kernel/plugin-view-host";

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
      {output.diagnostics.length ? (
        <Stack spacing={1} sx={{ m: 2 }} aria-label="Plugin diagnostics">
          {output.diagnostics.map((item) => (
            <Alert severity="warning" key={`${item.severity}:${item.code}:${item.message}`}>
              <strong>{item.severity.toUpperCase()}</strong> {item.message}
            </Alert>
          ))}
        </Stack>
      ) : null}
      <PluginViewHost
        output={output}
        resource={resource}
        gateway={gateway}
        onResourceChanged={onResourceChanged}
      />
    </Box>
  );
}

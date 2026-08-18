import { Alert, Stack } from "@mui/material";
import type { SxProps, Theme } from "@mui/material/styles";
import type { PluginDiagnostic } from "@/domain/plugin";

export function PluginDiagnostics({
  diagnostics,
  sx,
}: {
  diagnostics: readonly PluginDiagnostic[];
  sx?: SxProps<Theme>;
}) {
  if (diagnostics.length === 0) return null;
  return (
    <Stack spacing={1} sx={sx} aria-label="Plugin diagnostics">
      {diagnostics.map((item) => (
        <Alert severity={item.severity} key={`${item.severity}:${item.code}:${item.message}`}>
          <strong>{item.severity.toUpperCase()}</strong> {item.message}
        </Alert>
      ))}
    </Stack>
  );
}

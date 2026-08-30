import { Alert, Box, Button } from "@mui/material";
import type { PluginView } from "@/domain/plugin";
import type { PluginHostGateway } from "@/shared/api/gateways";

export default function DownloadRenderer({
  view,
  gateway,
}: {
  view: Extract<PluginView, { type: "download" }>;
  gateway: PluginHostGateway;
}) {
  const source = gateway.assetUrl(view.url);
  if (!source) {
    return (
      <Alert severity="error" sx={{ m: 2 }}>
        The plugin returned an invalid or external download URL.
      </Alert>
    );
  }
  return (
    <Box sx={{ display: "grid", placeItems: "center", p: 3 }}>
      <Button component="a" href={source} download={view.filename ?? ""} variant="contained">
        Download file
      </Button>
    </Box>
  );
}

import { Alert, Box, Button } from "@mui/material";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { PluginView } from "@/domain/plugin";

export default function DownloadRenderer({
  view,
  gateway,
}: {
  view: Extract<PluginView, { view: "download" }>;
  gateway: AssetGateway;
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

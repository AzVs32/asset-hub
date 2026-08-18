import { Alert, Box, Button } from "@mui/material";
import type { PluginViewRendererProps } from "@/kernel/plugin-kernel";

export default function DownloadRenderer({ view, gateway }: PluginViewRendererProps) {
  if (view.view !== "download") return null;
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

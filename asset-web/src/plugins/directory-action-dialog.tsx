import {
  Alert,
  Box,
  Button,
  Dialog,
  DialogContent,
  DialogTitle,
  Stack,
  Typography,
} from "@mui/material";
import ReactMarkdown from "react-markdown";
import { useGateway } from "@/application/ports/gateway-context";
import type { DirectoryActionOutput, PluginView } from "@/domain/plugin";
import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";
import { DirectoryPluginFrame } from "./directory-plugin-frame";

export interface DirectoryActionResult {
  directory: Directory;
  action: DirectoryAction;
  output: DirectoryActionOutput;
}

export function DirectoryActionDialog({
  result,
  onClose,
  onDirectoryChanged,
  onNavigate,
  onEditResource,
}: {
  result: DirectoryActionResult | null;
  onClose: () => void;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
  onEditResource?:
    | ((resource: Resource, action: ResourceAction) => void | Promise<void>)
    | undefined;
}) {
  return (
    <Dialog open={Boolean(result)} fullWidth maxWidth="lg" onClose={onClose}>
      <DialogTitle>{result?.action.label ?? "Directory action"}</DialogTitle>
      <DialogContent>
        {result ? (
          <>
            <Typography variant="caption" color="text.secondary" sx={{ display: "block", mb: 2 }}>
              {result.directory.path || "/"} · {result.action.id}
            </Typography>
            {result.output.diagnostics.length ? (
              <Stack spacing={1} sx={{ mb: 2 }}>
                {result.output.diagnostics.map((item) => (
                  <Alert severity="warning" key={`${item.code}:${item.message}`}>
                    {item.message}
                  </Alert>
                ))}
              </Stack>
            ) : null}
            {result.output.view ? (
              <DirectoryView
                result={result}
                view={result.output.view}
                onDirectoryChanged={onDirectoryChanged}
                onNavigate={onNavigate}
                onEditResource={onEditResource}
              />
            ) : null}
          </>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

function DirectoryView({
  result,
  view,
  onDirectoryChanged,
  onNavigate,
  onEditResource,
}: {
  result: DirectoryActionResult;
  view: PluginView;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
  onEditResource?:
    | ((resource: Resource, action: ResourceAction) => void | Promise<void>)
    | undefined;
}) {
  const gateway = useGateway();
  if (view.view === "text")
    return (
      <Box component="pre" sx={{ whiteSpace: "pre-wrap", p: 2.5, fontSize: "0.875rem", m: 0 }}>
        {view.text}
      </Box>
    );
  if (view.view === "markdown")
    return (
      <Box component="article" className="plugin-prose" sx={{ p: 2.5 }}>
        <ReactMarkdown>{view.markdown}</ReactMarkdown>
      </Box>
    );
  if (view.view === "json")
    return (
      <Box
        component="pre"
        sx={{
          maxHeight: "65vh",
          overflow: "auto",
          p: 2,
          fontFamily: "monospace",
          fontSize: "0.875rem",
          whiteSpace: "pre-wrap",
          m: 0,
        }}
      >
        {JSON.stringify(view.data, null, 2)}
      </Box>
    );
  if (view.view === "html")
    return (
      <Box
        component="iframe"
        sandbox=""
        title={view.title ?? "Directory action output"}
        srcDoc={view.html}
        sx={{ display: "block", height: "65vh", minHeight: "20rem", width: "100%", border: 0 }}
      />
    );
  if (view.view === "plugin_frame") {
    return (
      <Box sx={{ height: "70vh", minHeight: "24rem" }}>
        <DirectoryPluginFrame
          directory={result.directory}
          output={result.output}
          view={view}
          gateway={gateway}
          onDirectoryChanged={onDirectoryChanged}
          onNavigate={onNavigate}
          onEditResource={onEditResource}
        />
      </Box>
    );
  }
  if (view.view === "download") {
    const source = gateway.assetUrl(view.url);
    return source ? (
      <Button
        component="a"
        href={source}
        download={view.filename ?? ""}
        variant="text"
        sx={{ m: 2.5 }}
      >
        Download {view.filename ?? "file"}
      </Button>
    ) : null;
  }
  const source =
    view.encoding === "url"
      ? gateway.assetUrl(view.data)
      : `data:${view.mime_type};base64,${view.data}`;
  if (!source) return null;
  return view.mime_type.startsWith("image/") ? (
    <Box sx={{ p: 2.5 }}>
      <img
        style={{ maxHeight: "70vh", width: "100%", objectFit: "contain" }}
        src={source}
        alt={view.title ?? "Directory action media"}
      />
    </Box>
  ) : (
    // biome-ignore lint/a11y/useMediaCaption: the plugin media ABI does not expose transcript tracks
    <video style={{ maxHeight: "70vh", width: "100%" }} src={source} controls />
  );
}

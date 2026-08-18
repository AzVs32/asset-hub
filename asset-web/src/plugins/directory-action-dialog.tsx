import { Box, Dialog, DialogContent, DialogTitle, Typography } from "@mui/material";
import { useGateway } from "@/application/ports/gateway-context";
import type { DirectoryActionOutput, PluginView } from "@/domain/plugin";
import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";
import { DirectoryPluginFrame } from "./directory-plugin-frame";
import { PluginDiagnostics } from "./plugin-diagnostics";
import { GenericPluginViewRenderer } from "./renderers/generic-plugin-view";

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
            <PluginDiagnostics diagnostics={result.output.diagnostics} sx={{ mb: 2 }} />
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
  return <GenericPluginViewRenderer view={view} gateway={gateway} />;
}

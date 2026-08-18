import { Alert, Box, CircularProgress, Paper } from "@mui/material";
import { useQuery } from "@tanstack/react-query";
import type React from "react";
import { useGateway } from "@/application/ports/gateway-context";
import type { Directory, Resource, ResourceAction } from "@/domain/resource";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { DirectoryPluginFrame } from "@/plugins/directory-plugin-frame";
import { PluginDiagnostics } from "@/plugins/plugin-diagnostics";

export function DirectoryWorkspaceOutlet({
  directory,
  coreWorkspace,
  onDirectoryChanged,
  onNavigate,
  onEditResource,
  instanceVersion,
}: {
  directory: Directory | undefined;
  coreWorkspace: React.ReactNode;
  onDirectoryChanged: () => void | Promise<void>;
  onNavigate: (path: string) => void | Promise<void>;
  onEditResource: (resource: Resource, action: ResourceAction) => void | Promise<void>;
  instanceVersion: number;
}) {
  const gateway = useGateway();
  const kernel = usePluginKernel();
  const action = directory ? kernel.directoryWorkspaceAction(directory) : null;
  const result = useQuery({
    queryKey: ["directory-workspace", directory?.id, directory?.revision, action?.id],
    queryFn: () => {
      if (!directory || !action) throw new Error("Directory workspace provider is unavailable.");
      return gateway.executeDirectoryAction(directory, action.id);
    },
    enabled: Boolean(directory && action),
    retry: false,
  });

  if (!directory) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", p: 4 }}>
        <CircularProgress />
      </Box>
    );
  }
  if (!action) return coreWorkspace;
  if (result.error) {
    return (
      <Alert severity="error" sx={{ m: 2 }}>
        {result.error.message}
      </Alert>
    );
  }
  if (!result.data) {
    return (
      <Box sx={{ display: "flex", justifyContent: "center", p: 4 }}>
        <CircularProgress />
      </Box>
    );
  }
  const view = result.data.view;
  if (view?.view !== "plugin_frame") {
    return (
      <Alert severity="error" sx={{ m: 2 }}>
        Action {action.id} did not return a plugin_frame.
      </Alert>
    );
  }
  return (
    <Paper
      component="section"
      aria-label={action.label}
      sx={{
        m: 2.5,
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        flex: 1,
        overflow: "hidden",
      }}
    >
      <PluginDiagnostics diagnostics={result.data.diagnostics} sx={{ m: 2 }} />
      <DirectoryPluginFrame
        directory={directory}
        output={result.data}
        view={view}
        gateway={gateway}
        onDirectoryChanged={onDirectoryChanged}
        onNavigate={onNavigate}
        onEditResource={onEditResource}
        instanceVersion={instanceVersion}
      />
    </Paper>
  );
}

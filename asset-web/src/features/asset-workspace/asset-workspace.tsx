import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import { AppBar, Avatar, Box, Toolbar, Typography } from "@mui/material";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import type { Directory } from "@/domain/directory";
import type { Resource } from "@/domain/resource";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import { DirectoryDetail } from "./components/directory-detail";
import { DirectoryBreadcrumbs } from "./components/directory-navigation";
import { ResourceDetail } from "./components/resource-detail";
import { CreateFolderDialog, UploadResourceDialog } from "./components/resource-dialogs";
import { ResourceList } from "./components/resource-list";
import { useAssetWorkspaceCommands } from "./hooks/use-asset-workspace-commands";
import { useAssetWorkspaceListing } from "./hooks/use-asset-workspace-listing";

export function AssetWorkspace() {
  const gateway = useAssetWorkspaceGateway();
  const queryClient = useQueryClient();
  const browser = useAssetWorkspaceListing();
  const commands = useAssetWorkspaceCommands();
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [folderOpen, setFolderOpen] = React.useState(false);
  const selected = useQuery({
    queryKey: queryKeys.resource(browser.selectedId ?? ""),
    queryFn: () => gateway.findResource(browser.selectedId ?? ""),
    enabled: Boolean(browser.selectedId),
    refetchInterval: (query) => (query.state.data?.state.content === "pending" ? 1_000 : false),
  });
  const resource = browser.selectedId ? (selected.data ?? null) : null;
  const directory = browser.selectedDirectoryId
    ? ([browser.listing.data?.directory, ...(browser.listing.data?.folders ?? [])].find(
        (candidate): candidate is Directory => candidate?.id === browser.selectedDirectoryId,
      ) ?? null)
    : null;
  function selectResource(item: Resource) {
    queryClient.setQueryData(queryKeys.resource(item.id), item);
    browser.selectResource(item.id);
  }

  function downloadResource(item: Resource) {
    window.open(gateway.resourceDownloadUrl(item), "_blank", "noopener");
  }

  function deleteResource(item: Resource) {
    if (!window.confirm(`Delete ${item.name}?`)) return;
    commands.deleteResource.mutate(
      { resource: item },
      {
        onSuccess: () => {
          if (browser.selectedId === item.id) browser.selectResource(null);
        },
      },
    );
  }

  function downloadDirectory(item: Directory) {
    window.open(gateway.directoryDownloadUrl(item), "_blank", "noopener");
  }

  function deleteDirectory(item: Directory) {
    if (!window.confirm(`Delete empty directory ${item.name}?`)) return;
    commands.deleteDirectory.mutate(
      { directory: item },
      {
        onSuccess: () => {
          if (browser.selectedDirectoryId === item.id) browser.selectDirectory(null);
        },
      },
    );
  }

  return (
    <Box
      component="main"
      sx={{
        display: "flex",
        flexDirection: "column",
        height: { xs: "auto", lg: "100dvh" },
        minHeight: "100dvh",
        overflow: { xs: "visible", lg: "hidden" },
      }}
    >
      <AppBar position="static" color="transparent">
        <Toolbar sx={{ flexWrap: { xs: "wrap", lg: "nowrap" }, gap: 2, py: 1 }}>
          <Avatar sx={{ bgcolor: "primary.main" }}>
            <StorageRoundedIcon />
          </Avatar>
          <Box sx={{ minWidth: 0 }}>
            <Typography variant="h6" component="h1" noWrap>
              Asset Hub
            </Typography>
          </Box>
          <DirectoryBreadcrumbs
            path={browser.filters.directory}
            onNavigate={browser.openDirectory}
          />
        </Toolbar>
      </AppBar>
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: {
            xs: "1fr",
            lg: "minmax(0, 1fr) clamp(22rem, 30vw, 30rem)",
          },
          gridTemplateRows: { xs: "auto auto", lg: "minmax(0, 1fr)" },
          gap: 2,
          flex: 1,
          minHeight: 0,
          overflow: { xs: "visible", lg: "hidden" },
          p: 2,
        }}
      >
        <ResourceList
          listing={browser.listing.data}
          filters={browser.filters}
          selectedId={browser.selectedId}
          selectedDirectoryId={browser.selectedDirectoryId}
          loading={browser.listing.isFetching}
          error={browser.listing.error}
          onFilters={browser.updateFilters}
          onOpenDirectory={browser.openDirectory}
          onSelect={selectResource}
          onSelectDirectory={(item) => browser.selectDirectory(item.id)}
          onDownloadResource={downloadResource}
          onDeleteResource={deleteResource}
          onDownloadDirectory={downloadDirectory}
          onDeleteDirectory={deleteDirectory}
          onRefresh={() => void browser.listing.refetch()}
          onUpload={() => setUploadOpen(true)}
          onCreateFolder={() => setFolderOpen(true)}
        />
        {directory ? (
          <DirectoryDetail directory={directory} />
        ) : (
          <ResourceDetail
            resource={resource}
            pending={commands.update.isPending}
            onSave={(draft) => {
              if (!resource) return Promise.reject(new Error("Resource is unavailable"));
              return commands.update.mutateAsync({ resource, draft });
            }}
          />
        )}
      </Box>

      <UploadResourceDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        directory={browser.filters.directory}
        pending={commands.upload.isPending}
        progress={commands.uploadProgress}
        onUpload={(draft) => commands.upload.mutateAsync(draft)}
      />
      <CreateFolderDialog
        open={folderOpen}
        onOpenChange={setFolderOpen}
        parent={browser.filters.directory}
        pending={commands.createFolder.isPending}
        onCreate={(name) => {
          const parent = browser.listing.data?.directory;
          if (!parent || parent.path !== browser.filters.directory) {
            return Promise.reject(new Error("Parent directory is unavailable"));
          }
          return commands.createFolder.mutateAsync({ parent, name });
        }}
      />
    </Box>
  );
}

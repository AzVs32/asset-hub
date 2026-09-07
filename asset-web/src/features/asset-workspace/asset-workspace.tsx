import CloseIcon from "@mui/icons-material/Close";
import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import {
  Alert,
  AppBar,
  Avatar,
  Box,
  Button,
  CircularProgress,
  Drawer,
  IconButton,
  Toolbar,
  Typography,
  useMediaQuery,
} from "@mui/material";
import type { Theme } from "@mui/material/styles";
import { useQueryClient } from "@tanstack/react-query";
import React from "react";
import type { Directory } from "@/domain/directory";
import type { Resource } from "@/domain/resource";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import type { UploadDraft, UploadReceipt } from "@/shared/api/upload";
import { CreateFolderDialog } from "./components/create-folder-dialog";
import { DirectoryDetail } from "./components/directory-detail";
import { DirectoryNavigation } from "./components/directory-navigation";
import { ResourceDetail } from "./components/resource-detail";
import { UploadResourceDialog } from "./components/upload-resource-dialog";
import { UploadStatusList } from "./components/upload-status-list";
import { WorkspaceList } from "./components/workspace-list";
import { useAssetWorkspaceCommands } from "./hooks/use-asset-workspace-commands";
import { useAssetWorkspaceListing } from "./hooks/use-asset-workspace-listing";
import { useResourceDetail } from "./hooks/use-resource-detail";

export function AssetWorkspace() {
  const gateway = useAssetWorkspaceGateway();
  const queryClient = useQueryClient();
  const browser = useAssetWorkspaceListing();
  const commands = useAssetWorkspaceCommands();
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [folderOpen, setFolderOpen] = React.useState(false);
  const desktop = useMediaQuery((theme: Theme) => theme.breakpoints.up("md"));
  const selected = useResourceDetail(browser.selectedId);
  const [uploads, setUploads] = React.useState<UploadReceipt[]>(() => gateway.pendingUploads());
  const completeUpload = React.useCallback((id: string) => {
    setUploads((current) => current.filter((upload) => upload.id !== id));
  }, []);
  async function uploadResource(draft: UploadDraft) {
    let receipt: UploadReceipt | undefined;
    try {
      receipt = await commands.upload.mutateAsync(draft);
    } finally {
      const pending = gateway.pendingUploads();
      if (receipt) pending.push(receipt);
      setUploads((current) =>
        Array.from(new Map([...current, ...pending].map((upload) => [upload.id, upload])).values()),
      );
      for (const upload of pending)
        void queryClient.invalidateQueries({ queryKey: queryKeys.upload(upload.id) });
    }
  }
  const resource = browser.selectedId ? (selected.data ?? null) : null;
  const directory = browser.selectedDirectoryId
    ? ([browser.listing.data?.directory, ...(browser.listing.data?.folders ?? [])].find(
        (candidate): candidate is Directory => candidate?.id === browser.selectedDirectoryId,
      ) ?? null)
    : null;
  function selectResource(item: Resource) {
    queryClient.setQueryData<Resource>(queryKeys.resource(item.id), (current) =>
      current && current.revision > item.revision ? current : item,
    );
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
          browser.clearSelection("resource", item.id);
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
          browser.clearSelection("folder", item.id);
        },
      },
    );
  }

  function closeDetails() {
    if (browser.selectedId) browser.selectResource(null);
    else browser.selectDirectory(null);
  }
  const detail = browser.selectedDirectoryId ? (
    browser.listing.error ? (
      <Alert
        severity="error"
        action={
          <Button color="inherit" onClick={() => void browser.listing.refetch()}>
            Retry
          </Button>
        }
      >
        Unable to load folder details.
      </Alert>
    ) : browser.listing.isPending ? (
      <Box sx={{ p: 2 }}>
        <CircularProgress aria-label="Loading folder" />
      </Box>
    ) : directory ? (
      <DirectoryDetail directory={directory} />
    ) : (
      <Alert severity="info">This folder is no longer available in the current directory.</Alert>
    )
  ) : (
    <ResourceDetail
      resource={resource}
      selected={Boolean(browser.selectedId)}
      loading={selected.isPending}
      error={browser.selectedId ? selected.error : null}
      onRetry={() => void selected.refetch()}
      pending={commands.update.isPending}
      onSave={(snapshot, draft) => commands.update.mutateAsync({ resource: snapshot, draft })}
    />
  );

  return (
    <Box
      component="main"
      sx={{
        display: "flex",
        flexDirection: "column",
        height: { xs: "auto", md: "100dvh" },
        minHeight: "100dvh",
        overflow: { xs: "visible", md: "hidden" },
      }}
    >
      <AppBar position="static" color="transparent">
        <Toolbar
          sx={{
            minHeight: { xs: "auto", md: 76 },
            py: { xs: 1, md: 0 },
            px: { xs: 2, md: 3 },
            display: "grid",
            gridTemplateColumns: { xs: "auto 1fr", md: "auto minmax(20rem, 42rem) 1fr" },
            gridTemplateAreas: {
              xs: '"brand spacer" "navigation navigation"',
              md: '"brand navigation spacer"',
            },
            columnGap: { xs: 1, md: 2 },
            rowGap: 1,
          }}
        >
          <Box sx={{ gridArea: "brand", display: "flex", alignItems: "center", gap: 1.5 }}>
            <Avatar sx={{ bgcolor: "primary.main", width: 46, height: 46, flexShrink: 0 }}>
              <StorageRoundedIcon />
            </Avatar>
            <Typography variant="h5" component="h1" noWrap>
              Asset Hub
            </Typography>
          </Box>
          <DirectoryNavigation
            path={browser.filters.directory}
            onNavigate={browser.openDirectory}
            onRefresh={() => browser.listing.refetch()}
            onCreateFolder={() => setFolderOpen(true)}
            onUpload={() => setUploadOpen(true)}
          />
          <Box sx={{ gridArea: "spacer" }} />
        </Toolbar>
      </AppBar>
      <UploadStatusList uploads={uploads} onComplete={completeUpload} />
      <Box
        sx={{
          display: "grid",
          gridTemplateColumns: {
            xs: "1fr",
            md: "minmax(0, 1fr) clamp(22rem, 30vw, 30rem)",
          },
          gridTemplateRows: { xs: "auto", md: "minmax(0, 1fr)" },
          gap: 2,
          flex: 1,
          minHeight: 0,
          overflow: { xs: "visible", md: "hidden" },
          p: 2,
        }}
      >
        <WorkspaceList
          listing={browser.listing.data}
          filters={browser.filters}
          selectedId={browser.selectedId}
          selectedDirectoryId={browser.selectedDirectoryId}
          loading={browser.listing.isFetching}
          mutating={commands.deleteResource.isPending || commands.deleteDirectory.isPending}
          error={browser.listing.error}
          onFilters={browser.updateFilters}
          onOpenDirectory={browser.openDirectory}
          onSelect={selectResource}
          onSelectDirectory={(item) => browser.selectDirectory(item.id)}
          onDownloadResource={downloadResource}
          onDeleteResource={deleteResource}
          onDownloadDirectory={downloadDirectory}
          onDeleteDirectory={deleteDirectory}
        />
        {desktop ? (
          <Box
            sx={{ display: "flex", flexDirection: "column", minHeight: 0, "& > *": { flex: 1 } }}
          >
            {detail}
          </Box>
        ) : null}
      </Box>

      {!desktop ? (
        <Drawer
          anchor="right"
          open={Boolean(browser.selectedId || browser.selectedDirectoryId)}
          onClose={closeDetails}
          slotProps={{ paper: { sx: { width: { xs: "100%", sm: 480 }, p: 2 } } }}
        >
          <Box sx={{ display: "flex", justifyContent: "flex-end", mb: 1 }}>
            <IconButton aria-label="Close details" onClick={closeDetails}>
              <CloseIcon />
            </IconButton>
          </Box>
          {detail}
        </Drawer>
      ) : null}
      <UploadResourceDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        directory={browser.filters.directory}
        pending={commands.upload.isPending}
        progress={commands.uploadProgress}
        onUpload={uploadResource}
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

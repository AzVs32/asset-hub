import LogoutIcon from "@mui/icons-material/Logout";
import PeopleIcon from "@mui/icons-material/People";
import StorageRoundedIcon from "@mui/icons-material/StorageRounded";
import { AppBar, Avatar, Box, Button, IconButton, Toolbar, Typography } from "@mui/material";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";
import { useSession } from "@/features/auth/session-context";
import { useSignOut } from "@/features/auth/use-sign-out";
import { DirectoryActionDialog } from "@/plugins/directory-action-dialog";
import { PluginActionDialog } from "@/plugins/plugin-action-dialog";
import { DirectoryDetail } from "./components/directory-detail";
import { DirectoryBreadcrumbs, DirectoryKindEditor } from "./components/directory-navigation";
import { ResourceDetail } from "./components/resource-detail";
import { CreateFolderDialog, UploadResourceDialog } from "./components/resource-dialogs";
import { ResourceList } from "./components/resource-list";
import { CoreDirectoryWorkspace } from "./core-directory-workspace";
import { DirectoryWorkspaceOutlet } from "./directory-workspace-outlet";
import { useResourceCommands } from "./hooks/use-resource-commands";
import { useResourceListing } from "./hooks/use-resource-listing";

const UserAdministration = React.lazy(() =>
  import("@/features/users/user-administration").then((module) => ({
    default: module.UserAdministration,
  })),
);

export function ResourceWorkspace() {
  const gateway = useGateway();
  const user = useSession();
  const signOut = useSignOut();
  const queryClient = useQueryClient();
  const browser = useResourceListing();
  const commands = useResourceCommands();
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [folderOpen, setFolderOpen] = React.useState(false);
  const [usersOpen, setUsersOpen] = React.useState(false);
  const [directoryWorkspaceVersion, setDirectoryWorkspaceVersion] = React.useState(0);
  const selected = useQuery({
    queryKey: queryKeys.resource(browser.selectedId ?? ""),
    queryFn: () => gateway.findResource(browser.selectedId ?? ""),
    enabled: Boolean(browser.selectedId),
    refetchInterval: (query) =>
      query.state.data?.content?.verificationStatus === "pending" ? 1_000 : false,
  });
  const resource = browser.selectedId ? (selected.data ?? null) : null;
  const directory = browser.selectedDirectoryId
    ? ([browser.listing.data?.directory, ...(browser.listing.data?.folders ?? [])].find(
        (candidate): candidate is Directory => candidate?.id === browser.selectedDirectoryId,
      ) ?? null)
    : null;
  const formDirectory = browser.filters.directory || "/";
  const kinds = browser.kinds.data ?? [];
  const currentDirectory =
    browser.listing.data?.path === browser.filters.directory
      ? browser.listing.data.directory
      : undefined;
  function selectResource(item: Resource) {
    queryClient.setQueryData(queryKeys.resource(item.id), item);
    browser.selectResource(item.id);
  }

  function runResourceAction(item: Resource, action: ResourceAction) {
    const target = item.id === resource?.id ? resource : item;
    if (!confirmAction(action, target.name)) return;
    commands.execute.mutate(
      { resource: target, action },
      {
        onSuccess: (result) => {
          if (result.output.effects.includes("delete") && browser.selectedId === target.id) {
            browser.selectResource(null);
          }
        },
      },
    );
  }

  function runDirectoryAction(item: Directory, action: DirectoryAction) {
    if (!confirmAction(action, item.name)) return;
    commands.executeDirectory.mutate(
      { directory: item, action },
      {
        onSuccess: (result) => {
          if (result.output.effects.includes("delete") && browser.selectedDirectoryId === item.id) {
            browser.selectDirectory(null);
          }
        },
      },
    );
  }

  async function openResourceEditor(item: Resource, action: ResourceAction) {
    if (!confirmAction(action, item.name)) {
      throw new Error(`Action ${action.id} was not confirmed.`);
    }
    await commands.execute.mutateAsync({ resource: item, action });
  }

  async function logout() {
    try {
      await signOut();
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : "Sign out failed");
    }
  }

  return (
    <Box
      component="main"
      sx={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        overflow: "hidden",
      }}
    >
      <AppBar position="static" color="default">
        <Toolbar sx={{ gap: 2 }}>
          <Avatar sx={{ bgcolor: "primary.main" }}>
            <StorageRoundedIcon />
          </Avatar>
          <Box sx={{ minWidth: 0 }}>
            <Typography variant="h6" component="h1" noWrap>
              Asset Hub
            </Typography>
            <Typography variant="caption" color="text.secondary" noWrap>
              {user.username}
            </Typography>
          </Box>
          <DirectoryBreadcrumbs
            path={browser.filters.directory}
            onNavigate={browser.openDirectory}
          />
          <DirectoryKindEditor
            directory={currentDirectory}
            kinds={browser.directoryKinds.data ?? []}
            pending={commands.updateDirectoryKind.isPending}
            onKindChange={(kind) => {
              if (currentDirectory)
                commands.updateDirectoryKind.mutate({ directory: currentDirectory, kind });
            }}
          />
          {user.isAdmin ? (
            <Button color="inherit" startIcon={<PeopleIcon />} onClick={() => setUsersOpen(true)}>
              Users
            </Button>
          ) : null}
          <IconButton color="inherit" aria-label="Sign out" onClick={() => void logout()}>
            <LogoutIcon />
          </IconButton>
        </Toolbar>
      </AppBar>
      <DirectoryWorkspaceOutlet
        directory={currentDirectory}
        onDirectoryChanged={() => browser.listing.refetch().then(() => undefined)}
        onNavigate={browser.openDirectory}
        onEditResource={openResourceEditor}
        instanceVersion={directoryWorkspaceVersion}
        coreWorkspace={
          <CoreDirectoryWorkspace
            browser={
              <ResourceList
                listing={browser.listing.data}
                kinds={kinds}
                filters={browser.filters}
                selectedId={browser.selectedId}
                selectedDirectoryId={browser.selectedDirectoryId}
                loading={browser.listing.isFetching}
                error={browser.listing.error}
                onFilters={browser.updateFilters}
                onOpenDirectory={browser.openDirectory}
                onSelect={selectResource}
                onSelectDirectory={(item) => browser.selectDirectory(item.id)}
                onAction={runResourceAction}
                onRestore={(item) =>
                  commands.restore.mutate(item.id === resource?.id ? resource : item)
                }
                onDirectoryAction={runDirectoryAction}
                onRefresh={() => void browser.listing.refetch()}
                onUpload={() => setUploadOpen(true)}
                onCreateFolder={() => setFolderOpen(true)}
              />
            }
            detail={
              directory ? (
                <DirectoryDetail
                  directory={directory}
                  kind={
                    browser.directoryKinds.data?.find((item) => item.kind === directory.kind) ??
                    null
                  }
                />
              ) : (
                <ResourceDetail
                  resource={resource}
                  kinds={kinds}
                  pending={commands.update.isPending}
                  onSave={(draft) => {
                    if (!resource) return Promise.reject(new Error("Resource is unavailable"));
                    return commands.update.mutateAsync({ resource, draft });
                  }}
                />
              )
            }
          />
        }
      />

      <UploadResourceDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        directory={user.isAdmin && !browser.filters.directory ? "uploads" : formDirectory}
        kinds={kinds}
        pending={commands.upload.isPending}
        progress={commands.uploadProgress}
        onUpload={(draft) => commands.upload.mutateAsync(draft)}
      />
      <CreateFolderDialog
        open={folderOpen}
        onOpenChange={setFolderOpen}
        parent={browser.filters.directory}
        kinds={browser.directoryKinds.data ?? []}
        pending={commands.createFolder.isPending}
        onCreate={(name, kind) => {
          const parent = browser.listing.data?.directory;
          if (!parent || parent.path !== browser.filters.directory) {
            return Promise.reject(new Error("Parent directory is unavailable"));
          }
          return commands.createFolder.mutateAsync({
            parent,
            name,
            ...(kind ? { kind } : {}),
          });
        }}
      />
      <PluginActionDialog
        result={commands.actionResult}
        onClose={() => commands.setActionResult(null)}
        onResourceChanged={async () => {
          await commands.refresh(commands.actionResult?.resource.id);
          setDirectoryWorkspaceVersion((version) => version + 1);
        }}
      />
      <DirectoryActionDialog
        result={commands.directoryActionResult}
        onClose={() => commands.setDirectoryActionResult(null)}
        onDirectoryChanged={() => browser.listing.refetch().then(() => undefined)}
        onNavigate={browser.openDirectory}
        onEditResource={openResourceEditor}
      />
      {usersOpen ? (
        <React.Suspense fallback={null}>
          <UserAdministration
            open={usersOpen}
            onOpenChange={setUsersOpen}
            currentUserId={user.id}
          />
        </React.Suspense>
      ) : null}
    </Box>
  );
}

function confirmAction(action: ResourceAction | DirectoryAction, targetName: string): boolean {
  const message = action.ui.confirmation;
  return !message || window.confirm(message.replaceAll("{name}", targetName));
}

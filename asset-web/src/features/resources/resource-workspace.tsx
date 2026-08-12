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
import { ResourceDetail } from "./components/resource-detail";
import { CreateFolderDialog, UploadResourceDialog } from "./components/resource-dialogs";
import { ResourceList } from "./components/resource-list";
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

  async function logout() {
    try {
      await signOut();
    } catch (cause) {
      toast.error(cause instanceof Error ? cause.message : "Sign out failed");
    }
  }

  return (
    <main className="grid h-screen min-h-[42rem] bg-slate-100 lg:grid-cols-[minmax(0,1fr)_minmax(25rem,31rem)]">
      <ResourceList
        user={user}
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
        onRestore={(item) => commands.restore.mutate(item.id === resource?.id ? resource : item)}
        onDirectoryAction={runDirectoryAction}
        onRefresh={() => void browser.listing.refetch()}
        onUpload={() => setUploadOpen(true)}
        onCreateFolder={() => setFolderOpen(true)}
        onUsers={() => setUsersOpen(true)}
        onLogout={() => void logout()}
      />
      {directory ? (
        <DirectoryDetail
          directory={directory}
          kind={browser.directoryKinds.data?.find((item) => item.kind === directory.kind) ?? null}
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
      )}

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
        onResourceChanged={() => commands.refresh(commands.actionResult?.resource.id)}
      />
      <DirectoryActionDialog
        result={commands.directoryActionResult}
        onClose={() => commands.setDirectoryActionResult(null)}
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
    </main>
  );
}

function confirmAction(action: ResourceAction | DirectoryAction, targetName: string): boolean {
  const message = action.ui.confirmation;
  return !message || window.confirm(message.replaceAll("{name}", targetName));
}

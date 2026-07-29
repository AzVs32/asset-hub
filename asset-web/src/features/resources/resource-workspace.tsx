import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { Directory, Resource } from "@/domain/resource";
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
  });
  const resource = browser.selectedId ? (selected.data ?? null) : null;
  const directory = browser.selectedDirectoryId
    ? ([browser.listing.data?.directory, ...(browser.listing.data?.folders ?? [])].find(
        (candidate): candidate is Directory => candidate?.id === browser.selectedDirectoryId,
      ) ?? null)
    : null;
  const formDirectory = browser.filters.directory || "/";
  const kinds = browser.kinds.data ?? [];
  const busy =
    commands.update.isPending ||
    commands.remove.isPending ||
    commands.restore.isPending ||
    commands.execute.isPending ||
    commands.executeDirectory.isPending;

  function selectResource(item: Resource) {
    queryClient.setQueryData(queryKeys.resource(item.id), item);
    browser.selectResource(item.id);
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
        onAction={(item, action) => commands.execute.mutate({ resource: item, action })}
        onDirectoryAction={(directory, action) =>
          commands.executeDirectory.mutate({ directory, action })
        }
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
          pending={busy}
          onAction={(action) => commands.executeDirectory.mutate({ directory, action })}
        />
      ) : (
        <ResourceDetail
          resource={resource}
          kinds={kinds}
          pending={busy}
          onSave={(draft) => commands.update.mutateAsync({ id: resource?.id ?? "", draft })}
          onAction={(action) => {
            if (resource) commands.execute.mutate({ resource, action });
          }}
          onDelete={() => {
            if (resource && window.confirm(`Delete ${resource.name}?`))
              commands.remove.mutate(resource);
          }}
          onRestore={() => {
            if (resource) commands.restore.mutate(resource);
          }}
          onResourceChanged={() => commands.refresh(resource?.id)}
        />
      )}

      <UploadResourceDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        directory={user.isAdmin && !browser.filters.directory ? "uploads" : formDirectory}
        kinds={kinds}
        pending={commands.upload.isPending}
        onUpload={async (draft) => selectResource(await commands.upload.mutateAsync(draft))}
      />
      <CreateFolderDialog
        open={folderOpen}
        onOpenChange={setFolderOpen}
        parent={browser.filters.directory}
        kinds={browser.directoryKinds.data ?? []}
        pending={commands.createFolder.isPending}
        onCreate={(name, kind) =>
          commands.createFolder.mutateAsync({
            parent: browser.filters.directory,
            name,
            ...(kind ? { kind } : {}),
          })
        }
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

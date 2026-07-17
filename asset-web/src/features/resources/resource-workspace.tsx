import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { Resource } from "@/domain/resource";
import { useSession } from "@/features/auth/session-context";
import { PluginActionDialog } from "@/plugins/plugin-action-dialog";
import { ResourceDetail } from "./components/resource-detail";
import {
  CreateFolderDialog,
  CreateResourceDialog,
  UploadResourceDialog,
} from "./components/resource-dialogs";
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
  const queryClient = useQueryClient();
  const browser = useResourceListing();
  const commands = useResourceCommands();
  const [createOpen, setCreateOpen] = React.useState(false);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [folderOpen, setFolderOpen] = React.useState(false);
  const [usersOpen, setUsersOpen] = React.useState(false);
  const selected = useQuery({
    queryKey: queryKeys.resource(browser.selectedId ?? ""),
    queryFn: () => gateway.findResource(browser.selectedId ?? ""),
    enabled: Boolean(browser.selectedId),
  });
  const resource = browser.selectedId ? (selected.data ?? null) : null;
  const kinds = browser.kinds.data ?? [];
  const busy =
    commands.update.isPending ||
    commands.remove.isPending ||
    commands.restore.isPending ||
    commands.execute.isPending;

  function selectResource(item: Resource) {
    queryClient.setQueryData(queryKeys.resource(item.id), item);
    browser.selectResource(item.id);
  }

  async function logout() {
    try {
      await gateway.logout();
      queryClient.setQueryData(queryKeys.session, undefined);
      await queryClient.invalidateQueries({ queryKey: queryKeys.session });
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
        grants={browser.grants.data ?? []}
        filters={browser.filters}
        selectedId={browser.selectedId}
        loading={browser.listing.isFetching}
        error={browser.listing.error}
        onFilters={browser.updateFilters}
        onOpenDirectory={browser.openDirectory}
        onSelect={selectResource}
        onAction={(item, action) => commands.execute.mutate({ resource: item, action })}
        onRefresh={() => void browser.listing.refetch()}
        onCreate={() => setCreateOpen(true)}
        onUpload={() => setUploadOpen(true)}
        onCreateFolder={() => setFolderOpen(true)}
        onScan={() => commands.scan.mutate(browser.filters.directory)}
        onUsers={() => setUsersOpen(true)}
        onLogout={() => void logout()}
      />
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

      <CreateResourceDialog
        open={createOpen}
        onOpenChange={setCreateOpen}
        directory={browser.filters.directory}
        kinds={kinds}
        pending={commands.create.isPending}
        onCreate={async (draft) => selectResource(await commands.create.mutateAsync(draft))}
      />
      <UploadResourceDialog
        open={uploadOpen}
        onOpenChange={setUploadOpen}
        directory={browser.filters.directory || "uploads"}
        kinds={kinds}
        pending={commands.upload.isPending}
        onUpload={async (draft) => selectResource(await commands.upload.mutateAsync(draft))}
      />
      <CreateFolderDialog
        open={folderOpen}
        onOpenChange={setFolderOpen}
        parent={browser.filters.directory}
        pending={commands.createFolder.isPending}
        onCreate={(name) =>
          commands.createFolder.mutateAsync({ parent: browser.filters.directory, name })
        }
      />
      <PluginActionDialog
        result={commands.actionResult}
        onClose={() => commands.setActionResult(null)}
        onResourceChanged={() => commands.refresh(commands.actionResult?.resource.id)}
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

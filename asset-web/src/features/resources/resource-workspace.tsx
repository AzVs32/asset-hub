import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Database, LogOut, Users } from "lucide-react";
import React from "react";
import { toast } from "sonner";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";
import { useSession } from "@/features/auth/session-context";
import { useSignOut } from "@/features/auth/use-sign-out";
import { DirectoryActionDialog } from "@/plugins/directory-action-dialog";
import { PluginActionDialog } from "@/plugins/plugin-action-dialog";
import { Button } from "@/shared/ui/button";
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
    <main className="flex h-screen min-h-[42rem] flex-col overflow-hidden bg-[#eef2f7]">
      <header className="relative z-20 flex min-h-[4.75rem] shrink-0 items-center gap-5 border-b border-slate-200/80 bg-white/90 px-5 py-3 text-slate-900 shadow-[0_12px_35px_-26px_rgba(15,23,42,0.35)] backdrop-blur-xl xl:px-7">
        <div className="flex shrink-0 items-center gap-3">
          <span className="grid size-11 place-items-center rounded-2xl bg-gradient-to-br from-indigo-500 to-blue-500 text-white shadow-[0_10px_28px_-12px_rgba(99,102,241,0.95)] ring-1 ring-white/15">
            <Database size={20} strokeWidth={2.2} />
          </span>
          <div>
            <h1 className="font-bold tracking-[-0.025em] text-slate-950">Asset Hub</h1>
            <p className="text-xs font-medium text-slate-400">{user.username}</p>
          </div>
        </div>
        <DirectoryBreadcrumbs path={browser.filters.directory} onNavigate={browser.openDirectory} />
        <DirectoryKindEditor
          directory={currentDirectory}
          kinds={browser.directoryKinds.data ?? []}
          pending={commands.updateDirectoryKind.isPending}
          onKindChange={(kind) => {
            if (currentDirectory)
              commands.updateDirectoryKind.mutate({ directory: currentDirectory, kind });
          }}
        />
        <div className="ml-auto flex shrink-0 items-center gap-2">
          {user.isAdmin ? (
            <Button
              variant="ghost"
              size="small"
              className="text-slate-500 hover:bg-indigo-50 hover:text-indigo-700 focus-visible:ring-indigo-100"
              onClick={() => setUsersOpen(true)}
            >
              <Users size={16} />
              Users
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="icon"
            className="text-slate-400 hover:bg-slate-100 hover:text-slate-700 focus-visible:ring-slate-200"
            aria-label="Sign out"
            onClick={() => void logout()}
          >
            <LogOut size={18} />
          </Button>
        </div>
      </header>
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
    </main>
  );
}

function confirmAction(action: ResourceAction | DirectoryAction, targetName: string): boolean {
  const message = action.ui.confirmation;
  return !message || window.confirm(message.replaceAll("{name}", targetName));
}

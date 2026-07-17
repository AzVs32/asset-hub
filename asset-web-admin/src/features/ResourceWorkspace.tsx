import React from "react";
import { request } from "../api";
import type { CurrentUser, DirectoryAccessEntry } from "../api/contracts";
import { UserAdministration } from "../components/UserAdministration";
import { directoriesFromResources, emptyCreateDraft, emptyUploadDraft, errorMessage, normalizeDraftKind } from "../utils/resourceDrafts";
import { CreateResourceDialog } from "./resourceWorkspace/CreateResourceDialog";
import { PluginActionPanel } from "./resourceWorkspace/PluginActionPanel";
import { ResourceBrowser } from "./resourceWorkspace/ResourceBrowser";
import { ResourceDetailPanel } from "./resourceWorkspace/ResourceDetailPanel";
import { UploadResourceDialog } from "./resourceWorkspace/UploadResourceDialog";
import { useResourceListing } from "./resourceWorkspace/useResourceListing";
import { useResourceMutations } from "./resourceWorkspace/useResourceMutations";
import type { Draft, UploadDraft } from "./resourceWorkspace/models";

export function ResourceWorkspace({ initialDirectory = "", user, onLogout }: {
  initialDirectory?: string; user: CurrentUser; onLogout: () => Promise<void>;
}) {
  const listing = useResourceListing(initialDirectory);
  const mutations = useResourceMutations({
    currentDirectory: listing.currentDirectory,
    reload: listing.reload,
    setError: listing.setError,
  });
  const [createDraft, setCreateDraft] = React.useState<Draft>(emptyCreateDraft);
  const [uploadDraft, setUploadDraft] = React.useState<UploadDraft>(emptyUploadDraft);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [userAdminOpen, setUserAdminOpen] = React.useState(false);
  const [directoryEntries, setDirectoryEntries] = React.useState<DirectoryAccessEntry[]>(() => [{
    directory: user.is_admin ? "" : user.workspace_directory,
    permission: "full",
    is_workspace: !user.is_admin,
  }]);
  const [activeEntryDirectory, setActiveEntryDirectory] = React.useState(
    user.is_admin ? "" : user.workspace_directory,
  );

  React.useEffect(() => {
    if (user.is_admin) return;
    request<DirectoryAccessEntry[]>("/auth/directory-grants")
      .then((entries) => {
        if (entries.length) setDirectoryEntries(entries);
      })
      .catch((reason) => listing.setError(errorMessage(reason)));
  }, [listing.setError, user.is_admin]);

  const uploadDirectories = React.useMemo(() => Array.from(new Set([
    listing.currentDirectory || "uploads",
    ...listing.folders.map((folder) => folder.path),
    ...directoriesFromResources(listing.page.items),
  ])).sort(), [listing.currentDirectory, listing.folders, listing.page.items]);

  function openDirectory(path: string) {
    listing.openDirectory(path);
    mutations.select(null);
  }

  function changeDirectoryEntry(path: string) {
    setActiveEntryDirectory(path);
    openDirectory(path);
  }

  async function create(draft: Draft) {
    const resource = await mutations.create(draft);
    if (resource) {
      setCreateOpen(false);
      setCreateDraft(normalizeDraftKind(emptyCreateDraft(), listing.resourceKinds));
    }
  }

  async function upload(draft: UploadDraft) {
    const resource = await mutations.upload(draft);
    if (resource) {
      setUploadOpen(false);
      setUploadDraft(emptyUploadDraft());
    }
  }

  return <main className="grid min-h-screen bg-slate-100 lg:grid-cols-[minmax(0,1fr)_28rem]">
    <ResourceBrowser
      user={user} page={listing.page} folders={listing.folders} filters={listing.filters}
      directoryEntries={directoryEntries} activeEntryDirectory={activeEntryDirectory}
      onDirectoryEntryChange={changeDirectoryEntry}
      updateFilters={listing.updateFilters} setPage={(page) => listing.setFilters((current) => ({ ...current, page }))}
      currentDirectory={listing.currentDirectory} openDirectory={openDirectory} kinds={listing.resourceKinds}
      selected={mutations.selected} select={mutations.select} loading={listing.loading}
      scanPending={mutations.isPending("scan")} folderPending={mutations.isPending("create-folder")}
      error={listing.error} notice={mutations.notice} clearNotice={() => mutations.setNotice(null)}
      reload={() => void listing.reload()} onScan={() => void mutations.scan()} onUsers={() => setUserAdminOpen(true)}
      onAction={(resource, action) => void mutations.runAction(resource, action)}
      onCreate={() => { setCreateDraft(normalizeDraftKind({ ...emptyCreateDraft(), directory: listing.currentDirectory }, listing.resourceKinds)); setCreateOpen(true); }}
      onUpload={() => { setUploadDraft({ ...emptyUploadDraft(), directory: listing.currentDirectory || "uploads" }); setUploadOpen(true); }}
      onCreateFolder={mutations.createFolder} onLogout={onLogout}
    />
    <ResourceDetailPanel resource={mutations.selected} draft={mutations.draft} setDraft={mutations.setDraft}
      resourceKinds={listing.resourceKinds} busy={mutations.selected ? [...mutations.pendingOperations].some((key) => key.includes(mutations.selected!.id)) : false} onSave={() => void mutations.save()}
      onAction={(action) => mutations.selected && void mutations.runAction(mutations.selected, action)} onDelete={() => void mutations.remove()}
      onRestore={() => void mutations.restore()} />
    {createOpen && <CreateResourceDialog draft={createDraft} setDraft={setCreateDraft} kinds={listing.resourceKinds}
      busy={mutations.isPending("create")} onClose={() => setCreateOpen(false)} onSubmit={create} />}
    {uploadOpen && <UploadResourceDialog draft={uploadDraft} setDraft={setUploadDraft} kinds={listing.contentKinds}
      directories={uploadDirectories} busy={mutations.isPending("upload")} onClose={() => setUploadOpen(false)} onSubmit={upload} />}
    <PluginActionPanel
      result={mutations.actionResult}
      onClose={() => mutations.setActionResult(null)}
      onResourceChanged={mutations.refreshResource}
    />
    {userAdminOpen && <UserAdministration currentUserId={user.id} onClose={() => setUserAdminOpen(false)} />}
  </main>;
}

import React from "react";
import type { CurrentUser } from "../components/AuthGate";
import { UserAdministration } from "../components/UserAdministration";
import type { Draft, UploadDraft } from "../types";
import { directoriesFromResources, emptyCreateDraft, emptyUploadDraft, normalizeDraftKind } from "../utils/resourceDrafts";
import { CreateResourceDialog } from "./resourceWorkspace/CreateResourceDialog";
import { PluginActionPanel } from "./resourceWorkspace/PluginActionPanel";
import { ResourceBrowser } from "./resourceWorkspace/ResourceBrowser";
import { ResourceDetailPanel } from "./resourceWorkspace/ResourceDetailPanel";
import { ResourcePreviewDialog } from "./resourceWorkspace/ResourcePreviewDialog";
import { UploadResourceDialog } from "./resourceWorkspace/UploadResourceDialog";
import { useResourceListing } from "./resourceWorkspace/useResourceListing";
import { useResourceMutations } from "./resourceWorkspace/useResourceMutations";

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

  const uploadDirectories = React.useMemo(() => Array.from(new Set([
    listing.currentDirectory || "uploads",
    ...listing.folders.map((folder) => folder.path),
    ...directoriesFromResources(listing.page.items),
  ])).sort(), [listing.currentDirectory, listing.folders, listing.page.items]);

  function openDirectory(path: string) {
    listing.openDirectory(path);
    mutations.select(null);
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
      updateFilters={listing.updateFilters} setPage={(page) => listing.setFilters((current) => ({ ...current, page }))}
      currentDirectory={listing.currentDirectory} openDirectory={openDirectory} kinds={listing.resourceKinds}
      selected={mutations.selected} select={mutations.select} loading={listing.loading}
      scanPending={mutations.isPending("scan")} folderPending={mutations.isPending("create-folder")}
      error={listing.error} notice={mutations.notice} clearNotice={() => mutations.setNotice(null)}
      reload={() => void listing.reload()} onScan={() => void mutations.scan()} onUsers={() => setUserAdminOpen(true)}
      onCreate={() => { setCreateDraft(normalizeDraftKind({ ...emptyCreateDraft(), directory: listing.currentDirectory }, listing.resourceKinds)); setCreateOpen(true); }}
      onUpload={() => { setUploadDraft({ ...emptyUploadDraft(), directory: listing.currentDirectory || "uploads" }); setUploadOpen(true); }}
      onCreateFolder={mutations.createFolder} onLogout={onLogout}
    />
    <ResourceDetailPanel resource={mutations.selected} draft={mutations.draft} setDraft={mutations.setDraft}
      resourceKinds={listing.resourceKinds} busy={mutations.selected ? [...mutations.pendingOperations].some((key) => key.includes(mutations.selected!.id)) : false} onSave={() => void mutations.save()}
      onRead={() => void mutations.read()} onPreview={() => mutations.setPreviewResource(mutations.selected)}
      onPluginAction={(action) => void mutations.runAction(action)} onDelete={() => void mutations.remove()}
      onRestore={() => void mutations.restore()} />
    {createOpen && <CreateResourceDialog draft={createDraft} setDraft={setCreateDraft} kinds={listing.resourceKinds}
      busy={mutations.isPending("create")} onClose={() => setCreateOpen(false)} onSubmit={create} />}
    {uploadOpen && <UploadResourceDialog draft={uploadDraft} setDraft={setUploadDraft} kinds={listing.contentKinds}
      directories={uploadDirectories} busy={mutations.isPending("upload")} onClose={() => setUploadOpen(false)} onSubmit={upload} />}
    <ResourcePreviewDialog reader={mutations.reader} onClose={() => mutations.setReader(null)} />
    <ResourcePreviewDialog resource={mutations.previewResource} onClose={() => mutations.setPreviewResource(null)} />
    <PluginActionPanel output={mutations.pluginOutput} onClose={() => mutations.setPluginOutput(null)} />
    {userAdminOpen && <UserAdministration currentUserId={user.id} onClose={() => setUserAdminOpen(false)} />}
  </main>;
}

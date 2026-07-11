import React from "react";
import {
  ChevronLeft,
  ChevronRight,
  Database,
  File as FileIcon,
  FileUp,
  Folder,
  FolderPlus,
  Loader2,
  LogOut,
  Plus,
  RefreshCcw,
  Search,
  SlidersHorizontal,
  Users,
  X,
} from "lucide-react";
import { apiBase, request } from "../api";
import { PluginActionResult, PluginViewResult, pluginViewTitle } from "../components/PluginViewResult";
import { ResourceDetail } from "../components/ResourceDetail";
import { SelectInput, TextInput } from "../components/forms";
import type { CurrentUser } from "../components/AuthGate";
import { UserAdministration } from "../components/UserAdministration";
import { cx, iconButtonClass, inputClass, primaryButtonClass, secondaryButtonClass, textareaClass } from "../components/ui";
import type { DirectoryListing, Draft, Filters, PluginActionOutput, Resource, ResourceActionDefinition, ResourceDirectory, ResourceKindOption, ResourceKindsResponse, ResourcePage, ResourceReadResponse, ResourceStatus, ScanStorageResponse, UploadDraft } from "../types";
import { detectKindForFile, directoriesFromResources, emptyCreateDraft, emptyUploadDraft, errorMessage, fallbackUploadKind, formatBytes, formatDate, isImageResource, kindOptionLabel, metadataFromDraft, metadataFromUpload, normalizeDirectoryInput, normalizeDraftKind, sha256Hex, sortKindsForHierarchy, toDraft, withKindDefaults } from "../utils/resourceDrafts";

const fallbackKinds: ResourceKindOption[] = [
  {
    kind: "core:unknown",
    parent: null,
    ancestors: [],
    label: "core:unknown",
    schema_id: null,
    metadata_schema: null,
    supports_content: true,
    detect: undefined,
    actions: [],
    source: "builtin",
  },
];
const defaultFilters: Filters = {
  q: "",
  kind: "",
  tag: "",
  includeDeleted: false,
  includeDescendants: true,
  page: 1,
  limit: 20,
};
const modalBackdropClass = "fixed inset-0 z-50 grid place-items-center overflow-y-auto bg-slate-950/50 p-4 backdrop-blur-sm";
const modalClass = "max-h-[calc(100vh-2rem)] w-full max-w-2xl overflow-auto rounded-2xl bg-white shadow-2xl";
const modalHeaderClass = "flex items-center justify-between border-b border-slate-200 px-6 py-5";
const modalFormClass = "grid grid-cols-2 gap-4 p-6 max-sm:grid-cols-1";
const modalActionsClass = "col-span-full flex justify-end gap-2";

export function ResourceWorkspace({ initialDirectory = "", user, onLogout }: { initialDirectory?: string; user: CurrentUser; onLogout: () => Promise<void> }) {
  const [filters, setFilters] = React.useState<Filters>(defaultFilters);
  const [page, setPage] = React.useState<ResourcePage>({
    items: [],
    total: 0,
    page: 1,
    limit: defaultFilters.limit,
  });
  const [currentDirectory, setCurrentDirectory] = React.useState(initialDirectory);
  const [folders, setFolders] = React.useState<ResourceDirectory[]>([]);
  const [selected, setSelected] = React.useState<Resource | null>(null);
  const [draft, setDraft] = React.useState<Draft | null>(null);
  const [resourceKinds, setResourceKinds] = React.useState<ResourceKindOption[]>(fallbackKinds);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [folderOpen, setFolderOpen] = React.useState(false);
  const [folderName, setFolderName] = React.useState("");
  const [createDraft, setCreateDraft] = React.useState<Draft>(emptyCreateDraft);
  const [uploadOpen, setUploadOpen] = React.useState(false);
  const [uploadDraft, setUploadDraft] = React.useState<UploadDraft>(emptyUploadDraft);
  const [reader, setReader] = React.useState<ResourceReadResponse | null>(null);
  const [previewResource, setPreviewResource] = React.useState<Resource | null>(null);
  const [pluginOutput, setPluginOutput] = React.useState<PluginActionOutput | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [userAdminOpen, setUserAdminOpen] = React.useState(false);

  React.useEffect(() => {
    void loadResourceKinds();
  }, []);

  async function loadResourceKinds() {
    try {
      const result = await request<ResourceKindsResponse>("/resource-kinds");
      if (result.items.length > 0) {
        const kinds = sortKindsForHierarchy(result.items);
        const uploadKinds = kinds.filter((kind) => kind.supports_content);
        setResourceKinds(kinds);
        setCreateDraft((current) => normalizeDraftKind(current, kinds));
        setUploadDraft((current) => normalizeDraftKind(current, uploadKinds.length ? uploadKinds : kinds));
      }
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  const loadResources = React.useCallback(async () => {
    setLoading(true);
    setError(null);

    try {
      const params = new URLSearchParams({
        page: String(filters.page),
        limit: String(filters.limit),
        path: currentDirectory,
      });

      if (filters.q.trim()) params.set("q", filters.q.trim());
      if (filters.kind.trim()) params.set("kind", filters.kind.trim());
      if (filters.tag.trim()) params.set("tag", filters.tag.trim());
      if (filters.includeDeleted) params.set("include_deleted", "true");
      if (filters.kind && filters.includeDescendants) params.set("include_descendants", "true");

      const result = await request<DirectoryListing>(`/directories?${params.toString()}`);
      setFolders(result.folders);
      setPage(result.resources);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [currentDirectory, filters]);

  React.useEffect(() => {
    void loadResources();
  }, [loadResources]);

  const totalPages = Math.max(1, Math.ceil(page.total / page.limit));
  const contentKinds = React.useMemo(
    () => resourceKinds.filter((kind) => kind.supports_content),
    [resourceKinds],
  );
  const uploadDirectories = React.useMemo(
    () =>
      Array.from(
        new Set([
          currentDirectory || "uploads",
          ...folders.map((folder) => folder.path),
          ...directoriesFromResources(page.items),
        ]),
      ).sort((left, right) => left.localeCompare(right)),
    [currentDirectory, folders, page.items],
  );
  const breadcrumbs = React.useMemo(() => directoryBreadcrumbs(currentDirectory), [currentDirectory]);

  function updateFilters(patch: Partial<Filters>) {
    setFilters((current) => ({ ...current, ...patch, page: patch.page ?? 1 }));
  }

  function openDirectory(path: string) {
    setCurrentDirectory(path);
    setFilters((current) => ({ ...current, page: 1 }));
    setSelected(null);
    setDraft(null);
  }

  function selectResource(resource: Resource) {
    setSelected(resource);
    setDraft(toDraft(resource));
  }

  function updateUploadKind(kind: string) {
    setUploadDraft((current) => withKindDefaults({ ...current, kind }, contentKinds));
  }

  function inferUploadKind(file: File | null): string {
    if (!file) return fallbackUploadKind(contentKinds);
    return detectKindForFile(file, contentKinds);
  }

  function updateCreateKind(kind: string) {
    setCreateDraft((current) => withKindDefaults({ ...current, kind }, resourceKinds));
  }

  async function createResource(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);

    try {
      const created = await request<Resource>("/resources", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            name: createDraft.name,
            directory: normalizeDirectoryInput(createDraft.directory),
            kind: createDraft.kind,
            status: createDraft.status,
            metadata: metadataFromDraft(createDraft),
        }),
      });
      setCreateOpen(false);
      setCreateDraft(normalizeDraftKind(emptyCreateDraft(), resourceKinds));
      setSelected(created);
      setDraft(toDraft(created));
      setNotice("Created");
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function createFolder(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await request<ResourceDirectory>("/directories", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ parent_path: currentDirectory, name: folderName }),
      });
      setFolderOpen(false);
      setFolderName("");
      setNotice("Folder created");
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function saveSelected() {
    if (!selected || !draft) return;
    setBusy(true);
    setError(null);

    try {
      const updated = await request<Resource>(`/resources/${selected.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            name: draft.name,
            directory: normalizeDirectoryInput(draft.directory),
            kind: draft.kind,
            status: draft.status,
            metadata: metadataFromDraft(draft),
        }),
      });
      setSelected(updated);
      setDraft(toDraft(updated));
      setNotice("Saved");
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function softDeleteSelected() {
    if (!selected) return;
    setBusy(true);
    setError(null);

    try {
      const deleted = await request<Resource>(`/resources/${selected.id}`, {
        method: "DELETE",
      });
      setSelected(deleted);
      setDraft(toDraft(deleted));
      setNotice("Deleted");
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function restoreSelected() {
    if (!selected) return;
    setBusy(true);
    setError(null);

    try {
      const restored = await request<Resource>(`/resources/${selected.id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ restore: true, status: "active" }),
      });
      setSelected(restored);
      setDraft(toDraft(restored));
      setNotice("Restored");
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function readSelected() {
    if (!selected) return;

    setBusy(true);
    setError(null);

    try {
      const readable = await request<ResourceReadResponse>(`/resources/${selected.id}/read`);
      setReader(readable);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  function previewSelected() {
    if (!selected) return;
    setPreviewResource(selected);
  }

  async function runPluginAction(action: ResourceActionDefinition) {
    if (!selected) return;

    setBusy(true);
    setError(null);

    try {
      const output = await request<PluginActionOutput>(
        `/resources/${selected.id}/actions/${encodeURIComponent(action.id)}`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ input: {} }),
        },
      );
      setPluginOutput(output);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function uploadResource(event: React.FormEvent) {
    event.preventDefault();
    if (!uploadDraft.file) {
      setError("Select a file first");
      return;
    }

    setBusy(true);
    setError(null);

    try {
      const file = uploadDraft.file;
      const metadata = metadataFromUpload(uploadDraft);
      const params = new URLSearchParams({
        name: uploadDraft.name.trim() || file.name,
        directory: normalizeDirectoryInput(uploadDraft.directory),
        metadata_json: JSON.stringify(metadata),
        original_filename: file.name,
      });

      if (uploadDraft.kind.trim()) params.set("kind", uploadDraft.kind.trim());
      params.set("sha256", await sha256Hex(file));

      const uploaded = await request<Resource>(`/resources/content/stream?${params.toString()}`, {
        method: "PUT",
        headers: {
          "Content-Type": file.type || "application/octet-stream",
        },
        body: file,
      });

      setUploadOpen(false);
      setUploadDraft(emptyUploadDraft());
      setSelected(uploaded);
      setDraft(toDraft(uploaded));
      setNotice("Uploaded");
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function scanStorage() {
    setBusy(true);
    setError(null);
    setNotice(null);

    try {
      const result = await request<ScanStorageResponse>("/scan", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          path: currentDirectory,
          sha256: true,
        }),
      });
      setNotice(`Scanned ${result.scanned}, imported ${result.imported}, skipped ${result.skipped}`);
      await loadResources();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="grid min-h-screen bg-slate-100 lg:grid-cols-[minmax(0,1fr)_28rem]">
      <section className="flex min-w-0 flex-col border-r border-slate-200 bg-white" aria-label="Resources">
        <header className="flex min-h-20 items-center justify-between gap-4 border-b border-slate-200 px-6 py-4 max-md:flex-col max-md:items-stretch">
          <div className="flex min-w-0 items-center gap-3">
            <div className="flex size-10 items-center justify-center rounded-xl bg-blue-600 text-white"><Database size={20} aria-hidden="true" /></div>
            <div>
              <h1 className="text-lg font-bold tracking-tight text-slate-900">Asset Hub</h1>
              <span className="text-xs text-slate-500">{page.total} resources</span>
            </div>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2">
            <button className={iconButtonClass} type="button" onClick={loadResources} title="Refresh">
              {loading ? <Loader2 className="animate-spin" size={18} /> : <RefreshCcw size={18} />}
            </button>
            {user.is_admin && <button className={secondaryButtonClass} type="button" onClick={scanStorage} disabled={busy}>
              {busy ? <Loader2 className="animate-spin" size={18} /> : <Database size={18} />}Scan
            </button>}
            {user.is_admin && <button className={secondaryButtonClass} type="button" onClick={() => setUserAdminOpen(true)}><Users size={18} />Users</button>}
            <button
              className={secondaryButtonClass}
              type="button"
              onClick={() => {
                setFolderName("");
                setFolderOpen(true);
              }}
            >
              <FolderPlus size={18} />
              New folder
            </button>
            <button
              className={secondaryButtonClass}
              type="button"
              onClick={() => {
                setCreateDraft((draft) => ({ ...draft, directory: currentDirectory }));
                setCreateOpen(true);
              }}
            >
              <Plus size={18} />
              New
            </button>
            <button
              className={primaryButtonClass}
              type="button"
              onClick={() => {
                setUploadDraft((draft) => ({ ...draft, directory: currentDirectory || "uploads" }));
                setUploadOpen(true);
              }}
            >
              <FileUp size={18} />
              Upload
            </button>
            <button className={iconButtonClass} type="button" onClick={() => void onLogout()} title={`Sign out ${user.username}`}><LogOut size={18} /></button>
          </div>
        </header>

        <div className="grid gap-3 border-b border-slate-200 bg-slate-50 p-4 sm:grid-cols-2 xl:grid-cols-[minmax(220px,1fr)_minmax(180px,240px)_minmax(120px,180px)_auto_auto]">
          <label className="flex h-10 items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 focus-within:border-blue-500 focus-within:ring-4 focus-within:ring-blue-500/10">
            <Search size={17} aria-hidden="true" />
            <input className="w-full min-w-0 border-0 bg-transparent text-sm outline-none"
              value={filters.q}
              onChange={(event) => updateFilters({ q: event.target.value })}
              placeholder="Search name"
            />
          </label>
          <label className="flex h-10 items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 focus-within:border-blue-500 focus-within:ring-4 focus-within:ring-blue-500/10">
            <SlidersHorizontal size={16} aria-hidden="true" />
            <select className="w-full min-w-0 border-0 bg-transparent text-sm outline-none" value={filters.kind} onChange={(event) => updateFilters({ kind: event.target.value })}>
              <option value="">All kinds</option>
              {resourceKinds.map((kind) => (
                <option key={kind.kind} value={kind.kind}>
                  {kindOptionLabel(kind)}
                </option>
              ))}
            </select>
          </label>
          <label className="flex h-10 items-center rounded-lg border border-slate-300 bg-white px-3 focus-within:border-blue-500 focus-within:ring-4 focus-within:ring-blue-500/10">
            <input className="w-full min-w-0 border-0 bg-transparent text-sm outline-none"
              value={filters.tag}
              onChange={(event) => updateFilters({ tag: event.target.value })}
              placeholder="tag"
            />
          </label>
          <label className="flex h-10 items-center gap-2 whitespace-nowrap rounded-lg border border-slate-300 bg-white px-3 text-sm font-medium text-slate-700">
            <input className="size-4 rounded border-slate-300 text-blue-600"
              type="checkbox"
              checked={filters.includeDeleted}
              onChange={(event) => updateFilters({ includeDeleted: event.target.checked })}
            />
            Deleted
          </label>
          <label className="flex h-10 items-center gap-2 whitespace-nowrap rounded-lg border border-slate-300 bg-white px-3 text-sm font-medium text-slate-700">
            <input className="size-4 rounded border-slate-300 text-blue-600"
              type="checkbox"
              checked={filters.includeDescendants}
              disabled={!filters.kind}
              onChange={(event) => updateFilters({ includeDescendants: event.target.checked })}
            />
            Descendants
          </label>
        </div>

        {error && (
          <div className="mx-4 mt-3 rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700" role="alert">
            {error}
          </div>
        )}
        {notice && (
          <div className="mx-4 mt-3 rounded-lg border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-700" role="status" onAnimationEnd={() => setNotice(null)}>
            {notice}
          </div>
        )}

        <nav className="flex min-h-10 items-center gap-1 border-b border-slate-200 px-6 text-sm text-slate-500" aria-label="Current directory">
          {breadcrumbs.map((crumb, index) => (
            <React.Fragment key={crumb.path || "root"}>
              {index > 0 && <ChevronRight size={14} aria-hidden="true" />}
              <button className="font-medium text-blue-600 hover:underline" type="button" onClick={() => openDirectory(crumb.path)}>
                {crumb.label}
              </button>
            </React.Fragment>
          ))}
        </nav>

        <div className="flex min-h-0 flex-1 flex-col overflow-auto">
          {currentDirectory && (
            <button className="grid min-h-11 w-full grid-cols-[1.5rem_1fr] items-center gap-2 border-b border-slate-200 bg-slate-50 px-6 py-2 text-left text-sm font-semibold text-slate-600 hover:bg-blue-50" type="button" onClick={() => openDirectory(parentDirectory(currentDirectory))}>
              <Folder className="text-blue-600" size={18} aria-hidden="true" />
              <span className="truncate">..</span>
            </button>
          )}
          {folders.map((folder) => (
            <button key={folder.path} className="grid min-h-11 w-full grid-cols-[1.5rem_1fr] items-center gap-2 border-b border-slate-200 bg-slate-50 px-6 py-2 text-left text-sm font-semibold text-slate-600 hover:bg-blue-50" type="button" onClick={() => openDirectory(folder.path)}>
              <Folder className="text-blue-600" size={18} aria-hidden="true" />
              <span className="truncate">{folder.name}</span>
            </button>
          ))}
          {page.items.map((resource) => (
            <button
              key={resource.id}
              className={cx("grid min-h-20 w-full grid-cols-[3.5rem_minmax(0,1fr)_auto] items-center gap-4 border-b border-slate-200 px-6 py-3 text-left transition hover:bg-blue-50 max-sm:grid-cols-[3rem_minmax(0,1fr)_auto] max-sm:px-4", selected?.id === resource.id && "bg-blue-50 ring-1 ring-inset ring-blue-200")}
              type="button"
              onClick={() => selectResource(resource)}
            >
              {resource.actions.thumbnail ? (
                <img className="size-14 rounded-lg bg-slate-100 object-cover max-sm:size-12" src={`${apiBase}/resources/${resource.id}/thumbnail`} alt="" />
              ) : (
                <div className="flex size-14 items-center justify-center rounded-lg border border-dashed border-slate-300 bg-slate-50 text-slate-400 max-sm:size-12" aria-hidden="true">
                  <FileIcon size={18} />
                </div>
              )}
              <div className="min-w-0">
                <div className="mb-1.5 flex min-w-0 items-center gap-2">
                  <span className="truncate font-semibold text-slate-900">{resource.name}</span>
                  {resource.deleted_at && <span className="rounded-full bg-red-100 px-2 py-0.5 text-xs font-semibold text-red-700">deleted</span>}
                </div>
                <div className="flex min-w-0 gap-3 text-xs text-slate-500 max-sm:flex-wrap">
                  <span className="truncate">{resource.content?.key ?? "metadata"}</span>
                  <span className="truncate">{resource.kind}</span>
                  <span className="truncate">{formatBytes(resource.content?.size ?? 0)}</span>
                  <span className="truncate">{formatDate(resource.updated_at)}</span>
                </div>
              </div>
              <span className={cx("rounded-full px-2.5 py-1 text-xs font-semibold", resource.status === "active" ? "bg-emerald-100 text-emerald-700" : "bg-slate-200 text-slate-600")}>{resource.status}</span>
            </button>
          ))}
          {!loading && folders.length === 0 && page.items.length === 0 && <div className="grid min-h-48 place-items-center text-sm text-slate-500">No resources</div>}
          {loading && <div className="grid min-h-48 place-items-center text-sm text-slate-500">Loading</div>}
        </div>

        <footer className="flex min-h-16 items-center justify-end gap-3 border-t border-slate-200 bg-white px-6 py-3 text-sm text-slate-600">
          <button
            className={iconButtonClass}
            type="button"
            disabled={filters.page <= 1}
            onClick={() => setFilters((current) => ({ ...current, page: current.page - 1 }))}
            title="Previous"
          >
            <ChevronLeft size={18} />
          </button>
          <span>
            {filters.page} / {totalPages}
          </span>
          <button
            className={iconButtonClass}
            type="button"
            disabled={filters.page >= totalPages}
            onClick={() => setFilters((current) => ({ ...current, page: current.page + 1 }))}
            title="Next"
          >
            <ChevronRight size={18} />
          </button>
        </footer>
      </section>

      <aside className="min-w-0 bg-white max-lg:border-t max-lg:border-slate-200" aria-label="Resource detail">
        {selected && draft ? (
          <ResourceDetail
            resource={selected}
            draft={draft}
            setDraft={setDraft}
            resourceKinds={resourceKinds}
            busy={busy}
            onSave={saveSelected}
            onRead={readSelected}
            onPreview={previewSelected}
            onPluginAction={runPluginAction}
            onDelete={softDeleteSelected}
            onRestore={restoreSelected}
          />
        ) : (
          <div className="grid min-h-64 place-items-center p-8 text-slate-400 lg:min-h-screen">
            <div className="grid justify-items-center gap-3"><Database size={32} aria-hidden="true" />
            <span className="text-sm">Select a resource</span></div>
          </div>
        )}
      </aside>

      {reader && (
        <div className={modalBackdropClass} role="presentation">
          <section className={cx(modalClass, "max-w-4xl")} aria-label="Read resource">
            <header className={modalHeaderClass}>
              <div>
                <h2 className="text-xl font-bold text-slate-900">{reader.name}</h2>
                <span className="mt-1 block text-xs text-slate-500">
                  {reader.kind} / {reader.view.view}
                </span>
              </div>
              <button className={iconButtonClass} type="button" onClick={() => setReader(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            <PluginViewResult view={reader.view} title={reader.name} />
          </section>
        </div>
      )}

      {previewResource && (
        <div className={modalBackdropClass} role="presentation">
          <section className={cx(modalClass, "max-w-5xl")} aria-label="Preview resource">
            <header className={modalHeaderClass}>
              <div>
                <h2 className="text-xl font-bold text-slate-900">{previewResource.name}</h2>
                <span className="mt-1 block text-xs text-slate-500">{previewResource.kind} / preview</span>
              </div>
              <button className={iconButtonClass} type="button" onClick={() => setPreviewResource(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            {isImageResource(previewResource) ? (
              <div className="flex min-h-105 items-center justify-center bg-slate-50 p-6">
                <img
                  className="max-h-[72vh] max-w-full rounded-lg object-contain"
                  alt={previewResource.name}
                  src={`${apiBase}/resources/${previewResource.id}/preview`}
                />
              </div>
            ) : (
              <iframe
                className="block h-[72vh] w-full border-0 bg-slate-50"
                title={previewResource.name}
                src={`${apiBase}/resources/${previewResource.id}/preview`}
              />
            )}
          </section>
        </div>
      )}

      {pluginOutput && (
        <div className={modalBackdropClass} role="presentation">
          <section className={cx(modalClass, "max-w-4xl")} aria-label="Action result">
            <header className={modalHeaderClass}>
              <div>
                <h2 className="text-xl font-bold text-slate-900">{pluginViewTitle(pluginOutput.view) || pluginOutput.action}</h2>
                <span className="mt-1 block text-xs text-slate-500">{pluginOutput.action} / {pluginOutput.view.view}</span>
              </div>
              <button className={iconButtonClass} type="button" onClick={() => setPluginOutput(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            <PluginActionResult output={pluginOutput} />
          </section>
        </div>
      )}

      {createOpen && (
        <div className={modalBackdropClass} role="presentation">
          <section className={modalClass} aria-label="Create resource">
            <header className={modalHeaderClass}>
              <h2 className="text-xl font-bold text-slate-900">New resource</h2>
              <button className={iconButtonClass} type="button" onClick={() => setCreateOpen(false)} title="Close">
                <X size={18} />
              </button>
            </header>
            <form className={modalFormClass} onSubmit={createResource}>
              <TextInput
                label="Name"
                value={createDraft.name}
                onChange={(name) => setCreateDraft((draft) => ({ ...draft, name }))}
              />
              <TextInput
                label="Directory"
                value={createDraft.directory}
                onChange={(directory) => setCreateDraft((draft) => ({ ...draft, directory }))}
              />
              <SelectInput label="Kind" value={createDraft.kind} options={resourceKinds} onChange={updateCreateKind} />
              <label className="grid gap-2">
                <span className="text-xs font-semibold text-slate-600">Status</span>
                <select className={inputClass}
                  value={createDraft.status}
                  onChange={(event) =>
                    setCreateDraft((draft) => ({ ...draft, status: event.target.value as ResourceStatus }))
                  }
                >
                  <option value="active">active</option>
                  <option value="archived">archived</option>
                </select>
              </label>
              <TextInput
                label="Description"
                value={createDraft.description}
                onChange={(description) => setCreateDraft((draft) => ({ ...draft, description }))}
              />
              <TextInput
                label="Tags"
                value={createDraft.tags}
                onChange={(tags) => setCreateDraft((draft) => ({ ...draft, tags }))}
              />
              <TextInput
                label="Schema ID"
                value={createDraft.schemaId}
                onChange={(schemaId) => setCreateDraft((draft) => ({ ...draft, schemaId }))}
              />
              <label className="col-span-full grid gap-2">
                <span className="text-xs font-semibold text-slate-600">Kind data JSON</span>
                <textarea className={textareaClass}
                  value={createDraft.kindData}
                  onChange={(event) => setCreateDraft((draft) => ({ ...draft, kindData: event.target.value }))}
                  rows={6}
                />
              </label>
              <div className={modalActionsClass}>
                <button className={secondaryButtonClass} type="button" onClick={() => setCreateOpen(false)}>
                  Cancel
                </button>
                <button className={primaryButtonClass} type="submit" disabled={busy}>
                  {busy ? <Loader2 className="animate-spin" size={18} /> : <Plus size={18} />}
                  Create
                </button>
              </div>
            </form>
          </section>
        </div>
      )}

      {folderOpen && (
        <div className={modalBackdropClass} role="presentation">
          <section className={modalClass} aria-label="Create folder">
            <header className={modalHeaderClass}>
              <div>
                <h2 className="text-xl font-bold text-slate-900">New folder</h2>
                <span className="mt-1 block text-xs text-slate-500">{currentDirectory || "Root"}</span>
              </div>
              <button className={iconButtonClass} type="button" onClick={() => setFolderOpen(false)} title="Close">
                <X size={18} />
              </button>
            </header>
            <form className={modalFormClass} onSubmit={createFolder}>
              <TextInput label="Folder name" value={folderName} onChange={setFolderName} />
              <div className={modalActionsClass}>
                <button className={secondaryButtonClass} type="button" onClick={() => setFolderOpen(false)}>Cancel</button>
                <button className={primaryButtonClass} type="submit" disabled={busy || !folderName.trim()}>
                  {busy ? <Loader2 className="animate-spin" size={18} /> : <FolderPlus size={18} />}
                  Create folder
                </button>
              </div>
            </form>
          </section>
        </div>
      )}

      {uploadOpen && (
        <div className={modalBackdropClass} role="presentation">
          <section className={modalClass} aria-label="Upload resource">
            <header className={modalHeaderClass}>
              <h2 className="text-xl font-bold text-slate-900">Upload</h2>
              <button className={iconButtonClass} type="button" onClick={() => setUploadOpen(false)} title="Close">
                <X size={18} />
              </button>
            </header>
            <form className={modalFormClass} onSubmit={uploadResource}>
              <label className="col-span-full flex min-h-20 cursor-pointer items-center gap-3 rounded-xl border-2 border-dashed border-slate-300 bg-slate-50 p-4 text-sm font-medium text-slate-600 transition hover:border-blue-400 hover:bg-blue-50">
                <input className="sr-only"
                  type="file"
                  onChange={(event) => {
                    const file = event.target.files?.[0] ?? null;
                    const inferredKind = inferUploadKind(file);
                    setUploadDraft((current) => ({
                      ...current,
                      file,
                      name: current.name || file?.name || "",
                      kind: contentKinds.some((kind) => kind.kind === inferredKind) ? inferredKind : current.kind,
                    }));
                  }}
                />
                <FileUp size={22} aria-hidden="true" />
                <span>{uploadDraft.file?.name ?? "Choose file"}</span>
              </label>
              <TextInput label="Name" value={uploadDraft.name} onChange={(name) => setUploadDraft((d) => ({ ...d, name }))} />
              <SelectInput
                label="Kind"
                value={uploadDraft.kind}
                options={contentKinds}
                onChange={updateUploadKind}
              />
              <TextInput
                label="Directory"
                value={uploadDraft.directory}
                onChange={(directory) => setUploadDraft((d) => ({ ...d, directory }))}
                list="upload-directories"
              />
              <datalist id="upload-directories">
                {uploadDirectories.map((directory) => (
                  <option key={directory} value={directory} />
                ))}
              </datalist>
              <TextInput
                label="Description"
                value={uploadDraft.description}
                onChange={(description) => setUploadDraft((d) => ({ ...d, description }))}
              />
              <TextInput label="Tags" value={uploadDraft.tags} onChange={(tags) => setUploadDraft((d) => ({ ...d, tags }))} />
              <TextInput
                label="Schema ID"
                value={uploadDraft.schemaId}
                onChange={(schemaId) => setUploadDraft((d) => ({ ...d, schemaId }))}
              />
              <label className="col-span-full grid gap-2">
                <span className="text-xs font-semibold text-slate-600">Kind data JSON</span>
                <textarea className={textareaClass}
                  value={uploadDraft.kindData}
                  onChange={(event) => setUploadDraft((d) => ({ ...d, kindData: event.target.value }))}
                  rows={5}
                />
              </label>
              <div className={modalActionsClass}>
                <button className={secondaryButtonClass} type="button" onClick={() => setUploadOpen(false)}>
                  Cancel
                </button>
                <button className={primaryButtonClass} type="submit" disabled={busy}>
                  {busy ? <Loader2 className="animate-spin" size={18} /> : <FileUp size={18} />}
                  Upload
                </button>
              </div>
            </form>
          </section>
        </div>
      )}
      {userAdminOpen && <UserAdministration currentUserId={user.id} onClose={() => setUserAdminOpen(false)} />}
    </main>
  );
}

function directoryBreadcrumbs(directory: string): Array<{ label: string; path: string }> {
  const crumbs = [{ label: "Root", path: "" }];
  const parts = directory.split("/").filter(Boolean);
  for (let index = 0; index < parts.length; index += 1) {
    crumbs.push({
      label: parts[index],
      path: parts.slice(0, index + 1).join("/"),
    });
  }
  return crumbs;
}

function parentDirectory(directory: string): string {
  const parts = directory.split("/").filter(Boolean);
  parts.pop();
  return parts.join("/");
}

import React from "react";
import { createRoot } from "react-dom/client";
import {
  ChevronLeft,
  ChevronRight,
  Database,
  File as FileIcon,
  FileUp,
  Folder,
  Loader2,
  Plus,
  RefreshCcw,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-react";
import "./styles.css";
import { apiBase, request } from "./api";
import { PluginActionResult, PluginViewResult, pluginViewTitle } from "./components/PluginViewResult";
import { ResourceDetail } from "./components/ResourceDetail";
import { SelectInput, TextInput } from "./components/forms";
import type { Draft, Filters, PluginActionOutput, Resource, ResourceActionDefinition, ResourceKindOption, ResourceKindsResponse, ResourcePage, ResourceReadResponse, ResourceStatus, UploadDraft } from "./types";
import { buildResourceTreeRows, detectKindForFile, directoriesFromResources, emptyCreateDraft, emptyUploadDraft, errorMessage, fallbackUploadKind, formatBytes, formatDate, isImageResource, metadataFromDraft, metadataFromUpload, normalizeDirectoryInput, normalizeDraftKind, sha256Hex, toDraft, withKindDefaults } from "./utils/resourceDrafts";

const fallbackKinds: ResourceKindOption[] = [
  {
    kind: "core:unknown",
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
  page: 1,
  limit: 20,
};

function App() {
  const [filters, setFilters] = React.useState<Filters>(defaultFilters);
  const [page, setPage] = React.useState<ResourcePage>({
    items: [],
    total: 0,
    page: 1,
    limit: defaultFilters.limit,
  });
  const [selected, setSelected] = React.useState<Resource | null>(null);
  const [draft, setDraft] = React.useState<Draft | null>(null);
  const [resourceKinds, setResourceKinds] = React.useState<ResourceKindOption[]>(fallbackKinds);
  const [createOpen, setCreateOpen] = React.useState(false);
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

  React.useEffect(() => {
    void loadResourceKinds();
  }, []);

  async function loadResourceKinds() {
    try {
      const result = await request<ResourceKindsResponse>("/resource-kinds");
      if (result.items.length > 0) {
        const uploadKinds = result.items.filter((kind) => kind.supports_content);
        setResourceKinds(result.items);
        setCreateDraft((current) => normalizeDraftKind(current, result.items));
        setUploadDraft((current) => normalizeDraftKind(current, uploadKinds.length ? uploadKinds : result.items));
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
      });

      if (filters.q.trim()) params.set("q", filters.q.trim());
      if (filters.kind.trim()) params.set("kind", filters.kind.trim());
      if (filters.tag.trim()) params.set("tag", filters.tag.trim());
      if (filters.includeDeleted) params.set("include_deleted", "true");

      const result = await request<ResourcePage>(`/resources?${params.toString()}`);
      setPage(result);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [filters]);

  React.useEffect(() => {
    void loadResources();
  }, [loadResources]);

  const totalPages = Math.max(1, Math.ceil(page.total / page.limit));
  const contentKinds = React.useMemo(
    () => resourceKinds.filter((kind) => kind.supports_content),
    [resourceKinds],
  );
  const resourceRows = React.useMemo(() => buildResourceTreeRows(page.items), [page.items]);
  const uploadDirectories = React.useMemo(() => directoriesFromResources(page.items), [page.items]);

  function updateFilters(patch: Partial<Filters>) {
    setFilters((current) => ({ ...current, ...patch, page: patch.page ?? 1 }));
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

  return (
    <main className="app-shell">
      <section className="resource-browser" aria-label="Resources">
        <header className="topbar">
          <div className="brand">
            <Database size={22} aria-hidden="true" />
            <div>
              <h1>Asset Hub</h1>
              <span>{page.total} resources</span>
            </div>
          </div>
          <div className="topbar-actions">
            <button className="icon-button" type="button" onClick={loadResources} title="Refresh">
              {loading ? <Loader2 className="spin" size={18} /> : <RefreshCcw size={18} />}
            </button>
            <button className="ghost-button" type="button" onClick={() => setCreateOpen(true)}>
              <Plus size={18} />
              New
            </button>
            <button className="primary-button" type="button" onClick={() => setUploadOpen(true)}>
              <FileUp size={18} />
              Upload
            </button>
          </div>
        </header>

        <div className="filter-row">
          <label className="search-field">
            <Search size={17} aria-hidden="true" />
            <input
              value={filters.q}
              onChange={(event) => updateFilters({ q: event.target.value })}
              placeholder="Search name"
            />
          </label>
          <label className="compact-field">
            <SlidersHorizontal size={16} aria-hidden="true" />
            <select value={filters.kind} onChange={(event) => updateFilters({ kind: event.target.value })}>
              <option value="">All kinds</option>
              {resourceKinds.map((kind) => (
                <option key={kind.kind} value={kind.kind}>
                  {kind.label}
                </option>
              ))}
            </select>
          </label>
          <label className="compact-field">
            <input
              value={filters.tag}
              onChange={(event) => updateFilters({ tag: event.target.value })}
              placeholder="tag"
            />
          </label>
          <label className="toggle-field">
            <input
              type="checkbox"
              checked={filters.includeDeleted}
              onChange={(event) => updateFilters({ includeDeleted: event.target.checked })}
            />
            Deleted
          </label>
        </div>

        {error && (
          <div className="message error-message" role="alert">
            {error}
          </div>
        )}
        {notice && (
          <div className="message notice-message" role="status" onAnimationEnd={() => setNotice(null)}>
            {notice}
          </div>
        )}

        <div className="resource-list">
          {resourceRows.map((row) =>
            row.type === "folder" ? (
              <div
                key={`folder:${row.path}`}
                className="folder-row"
                style={{ "--depth": row.depth } as React.CSSProperties}
              >
                <Folder size={18} aria-hidden="true" />
                <span>{row.name}</span>
                <small>{row.count}</small>
              </div>
            ) : (
              <button
                key={row.resource.id}
                className={`resource-row ${selected?.id === row.resource.id ? "selected" : ""}`}
                style={{ "--depth": row.depth } as React.CSSProperties}
                type="button"
                onClick={() => selectResource(row.resource)}
              >
                {row.resource.actions.thumbnail ? (
                  <img className="row-thumbnail" src={`${apiBase}/resources/${row.resource.id}/thumbnail`} alt="" />
                ) : (
                  <div className="row-thumbnail placeholder" aria-hidden="true">
                    <FileIcon size={18} />
                  </div>
                )}
                <div className="row-main">
                  <div className="row-title">
                    <span>{row.resource.name}</span>
                    {row.resource.deleted_at && <span className="deleted-pill">deleted</span>}
                  </div>
                  <div className="row-meta">
                    <span>{row.resource.content?.key ?? "metadata"}</span>
                    <span>{row.resource.kind}</span>
                    <span>{formatBytes(row.resource.content?.size ?? 0)}</span>
                    <span>{formatDate(row.resource.updated_at)}</span>
                  </div>
                </div>
                <span className={`status-pill ${row.resource.status}`}>{row.resource.status}</span>
              </button>
            ),
          )}
          {!loading && page.items.length === 0 && <div className="empty-state">No resources</div>}
          {loading && <div className="empty-state">Loading</div>}
        </div>

        <footer className="pager">
          <button
            className="icon-button"
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
            className="icon-button"
            type="button"
            disabled={filters.page >= totalPages}
            onClick={() => setFilters((current) => ({ ...current, page: current.page + 1 }))}
            title="Next"
          >
            <ChevronRight size={18} />
          </button>
        </footer>
      </section>

      <aside className="detail-panel" aria-label="Resource detail">
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
          <div className="detail-empty">
            <Database size={32} aria-hidden="true" />
            <span>Select a resource</span>
          </div>
        )}
      </aside>

      {reader && (
        <div className="modal-backdrop" role="presentation">
          <section className="modal reader-modal" aria-label="Read resource">
            <header className="modal-header">
              <div>
                <h2>{reader.name}</h2>
                <span>
                  {reader.kind} / {reader.view.view}
                </span>
              </div>
              <button className="icon-button" type="button" onClick={() => setReader(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            <PluginViewResult view={reader.view} title={reader.name} />
          </section>
        </div>
      )}

      {previewResource && (
        <div className="modal-backdrop" role="presentation">
          <section className="modal pdf-modal" aria-label="Preview resource">
            <header className="modal-header">
              <div>
                <h2>{previewResource.name}</h2>
                <span>{previewResource.kind} / preview</span>
              </div>
              <button className="icon-button" type="button" onClick={() => setPreviewResource(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            {isImageResource(previewResource) ? (
              <div className="image-preview-shell">
                <img
                  className="image-preview"
                  alt={previewResource.name}
                  src={`${apiBase}/resources/${previewResource.id}/preview`}
                />
              </div>
            ) : (
              <iframe
                className="pdf-frame"
                title={previewResource.name}
                src={`${apiBase}/resources/${previewResource.id}/preview`}
              />
            )}
          </section>
        </div>
      )}

      {pluginOutput && (
        <div className="modal-backdrop" role="presentation">
          <section className="modal plugin-modal" aria-label="Action result">
            <header className="modal-header">
              <div>
                <h2>{pluginViewTitle(pluginOutput.view) || pluginOutput.action}</h2>
                <span>{pluginOutput.action} / {pluginOutput.view.view}</span>
              </div>
              <button className="icon-button" type="button" onClick={() => setPluginOutput(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            <PluginActionResult output={pluginOutput} />
          </section>
        </div>
      )}

      {createOpen && (
        <div className="modal-backdrop" role="presentation">
          <section className="modal" aria-label="Create resource">
            <header className="modal-header">
              <h2>New resource</h2>
              <button className="icon-button" type="button" onClick={() => setCreateOpen(false)} title="Close">
                <X size={18} />
              </button>
            </header>
            <form className="form-grid" onSubmit={createResource}>
              <TextInput
                label="Name"
                value={createDraft.name}
                onChange={(name) => setCreateDraft((draft) => ({ ...draft, name }))}
              />
              <SelectInput label="Kind" value={createDraft.kind} options={resourceKinds} onChange={updateCreateKind} />
              <label className="field">
                <span>Status</span>
                <select
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
              <label className="field full">
                <span>Kind data JSON</span>
                <textarea
                  value={createDraft.kindData}
                  onChange={(event) => setCreateDraft((draft) => ({ ...draft, kindData: event.target.value }))}
                  rows={6}
                />
              </label>
              <div className="modal-actions">
                <button className="ghost-button" type="button" onClick={() => setCreateOpen(false)}>
                  Cancel
                </button>
                <button className="primary-button" type="submit" disabled={busy}>
                  {busy ? <Loader2 className="spin" size={18} /> : <Plus size={18} />}
                  Create
                </button>
              </div>
            </form>
          </section>
        </div>
      )}

      {uploadOpen && (
        <div className="modal-backdrop" role="presentation">
          <section className="modal" aria-label="Upload resource">
            <header className="modal-header">
              <h2>Upload</h2>
              <button className="icon-button" type="button" onClick={() => setUploadOpen(false)} title="Close">
                <X size={18} />
              </button>
            </header>
            <form className="form-grid" onSubmit={uploadResource}>
              <label className="file-drop">
                <input
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
              <label className="field full">
                <span>Kind data JSON</span>
                <textarea
                  value={uploadDraft.kindData}
                  onChange={(event) => setUploadDraft((d) => ({ ...d, kindData: event.target.value }))}
                  rows={5}
                />
              </label>
              <div className="modal-actions">
                <button className="ghost-button" type="button" onClick={() => setUploadOpen(false)}>
                  Cancel
                </button>
                <button className="primary-button" type="submit" disabled={busy}>
                  {busy ? <Loader2 className="spin" size={18} /> : <FileUp size={18} />}
                  Upload
                </button>
              </div>
            </form>
          </section>
        </div>
      )}
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

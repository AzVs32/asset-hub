import React from "react";
import { createRoot } from "react-dom/client";
import {
  BookOpen,
  ChevronLeft,
  ChevronRight,
  Database,
  Download,
  Eye,
  File as FileIcon,
  FileUp,
  Folder,
  Loader2,
  Plus,
  RefreshCcw,
  RotateCcw,
  Save,
  Search,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import "./styles.css";

type ResourceStatus = "active" | "archived";

type ResourceMetadata = {
  schema_version: number;
  summary: {
    description: string | null;
    tags: string[];
  };
  kind: {
    schema_id: string;
    data: Record<string, unknown>;
  } | null;
};

type ResourceContent = {
  key: string;
  size: number;
  mime_type: string | null;
  original_filename: string | null;
  checksum: Array<{ kind: string; value: string }>;
};

type ResourceActions = {
  download_content: boolean;
  read: boolean;
  view_inline: boolean;
  preview: boolean;
  thumbnail: boolean;
  available_actions: ResourceActionDefinition[];
};

type ResourceActionDefinition = {
  id: string;
  label: string;
  access: "read_only" | "read_write";
  when?: {
    mime_types: string[];
    extensions: string[];
  };
};

type Resource = {
  id: string;
  name: string;
  kind: string;
  status: ResourceStatus;
  metadata: ResourceMetadata;
  content: ResourceContent | null;
  actions: ResourceActions;
  created_at: string;
  updated_at: string;
  deleted_at: string | null;
};

type ResourceKindOption = {
  kind: string;
  label: string;
  schema_id: string | null;
  metadata_schema: Record<string, unknown> | null;
  supports_content: boolean;
  detect?: {
    mime_types: string[];
    extensions: string[];
  };
  actions: ResourceActionDefinition[];
  source: string;
};

type ResourceReadResponse = {
  id: string;
  name: string;
  kind: string;
  format: string;
  text: string;
};

type PluginActionOutput = {
  resource_id: string;
  action: string;
  content_type: string;
  body: PluginActionBody;
};

type PluginActionBody = {
  view?: "html" | "markdown" | "text" | "json" | "binary_url";
  title?: string;
  content?: unknown;
  html?: string;
  markdown?: string;
  text?: string;
  url?: string;
  [key: string]: unknown;
};

type ResourceKindsResponse = {
  items: ResourceKindOption[];
};

type ResourcePage = {
  items: Resource[];
  total: number;
  page: number;
  limit: number;
};

type Filters = {
  q: string;
  kind: string;
  tag: string;
  includeDeleted: boolean;
  page: number;
  limit: number;
};

type Draft = {
  name: string;
  kind: string;
  status: ResourceStatus;
  description: string;
  tags: string;
  schemaId: string;
  kindData: string;
};

type UploadDraft = {
  file: File | null;
  name: string;
  kind: string;
  directory: string;
  description: string;
  tags: string;
  schemaId: string;
  kindData: string;
};

type ResourceTreeRow =
  | {
      type: "folder";
      path: string;
      name: string;
      depth: number;
      count: number;
    }
  | {
      type: "resource";
      resource: Resource;
      depth: number;
    };

const apiBase = import.meta.env.VITE_API_BASE_URL || "/api";
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
                  {reader.kind} / {reader.format}
                </span>
              </div>
              <button className="icon-button" type="button" onClick={() => setReader(null)} title="Close">
                <X size={18} />
              </button>
            </header>
            <article className="reader-content">{reader.text}</article>
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
                <h2>{pluginOutput.body.title || pluginOutput.action}</h2>
                <span>{pluginOutput.action} / {pluginOutput.body.view || "json"}</span>
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

function ResourceDetail({
  resource,
  draft,
  setDraft,
  resourceKinds,
  busy,
  onSave,
  onRead,
  onPreview,
  onPluginAction,
  onDelete,
  onRestore,
}: {
  resource: Resource;
  draft: Draft;
  setDraft: React.Dispatch<React.SetStateAction<Draft | null>>;
  resourceKinds: ResourceKindOption[];
  busy: boolean;
  onSave: () => void;
  onRead: () => void;
  onPreview: () => void;
  onPluginAction: (action: ResourceActionDefinition) => void;
  onDelete: () => void;
  onRestore: () => void;
}) {
  const kindDefinition = resourceKinds.find((kind) => kind.kind === resource.kind);
  const canRead = resource.actions.read;
  const canPreview = resource.actions.preview || resource.actions.view_inline;
  const pluginActions = resource.actions.available_actions.filter(isPluginUiAction);

  return (
    <div className="detail-content">
      <header className="detail-header">
        <div>
          <h2>{resource.name}</h2>
          <span>{resource.id}</span>
        </div>
        <span className={`status-pill ${resource.status}`}>{resource.status}</span>
      </header>

      <div className="detail-actions">
        <button className="primary-button" type="button" onClick={onSave} disabled={busy || Boolean(resource.deleted_at)}>
          {busy ? <Loader2 className="spin" size={18} /> : <Save size={18} />}
          Save
        </button>
        {resource.actions.download_content && (
          <a className="icon-button" href={`${apiBase}/resources/${resource.id}/content`} title="Download">
            <Download size={18} />
          </a>
        )}
        {canRead && (
          <button className="icon-button" type="button" onClick={onRead} disabled={busy} title="Read">
            <BookOpen size={18} />
          </button>
        )}
        {canPreview && (
          <button className="icon-button" type="button" onClick={onPreview} disabled={busy} title="Preview">
            <Eye size={18} />
          </button>
        )}
        {pluginActions.map((action) => (
          <button
            key={action.id}
            className="ghost-button"
            type="button"
            onClick={() => onPluginAction(action)}
            disabled={busy}
            title={`${action.label} (${action.access})`}
          >
            {action.label}
          </button>
        ))}
        {resource.deleted_at ? (
          <button className="icon-button" type="button" onClick={onRestore} disabled={busy} title="Restore">
            <RotateCcw size={18} />
          </button>
        ) : (
          <button className="icon-button danger" type="button" onClick={onDelete} disabled={busy} title="Delete">
            <Trash2 size={18} />
          </button>
        )}
      </div>

      <div className="form-grid detail-form">
        <TextInput label="Name" value={draft.name} onChange={(name) => setDraft((d) => d && { ...d, name })} />
        <SelectInput
          label="Kind"
          value={draft.kind}
          options={resourceKinds}
          onChange={(kind) => setDraft((d) => d && withKindDefaults({ ...d, kind }, resourceKinds))}
        />
        <label className="field">
          <span>Status</span>
          <select
            value={draft.status}
            onChange={(event) => setDraft((d) => d && { ...d, status: event.target.value as ResourceStatus })}
            disabled={Boolean(resource.deleted_at)}
          >
            <option value="active">active</option>
            <option value="archived">archived</option>
          </select>
        </label>
        <TextInput
          label="Description"
          value={draft.description}
          onChange={(description) => setDraft((d) => d && { ...d, description })}
        />
        <TextInput label="Tags" value={draft.tags} onChange={(tags) => setDraft((d) => d && { ...d, tags })} />
        <TextInput
          label="Schema ID"
          value={draft.schemaId}
          onChange={(schemaId) => setDraft((d) => d && { ...d, schemaId })}
        />
        <label className="field full">
          <span>Kind data JSON</span>
          <textarea
            value={draft.kindData}
            onChange={(event) => setDraft((d) => d && { ...d, kindData: event.target.value })}
            rows={7}
            disabled={Boolean(resource.deleted_at)}
          />
        </label>
      </div>

      <section className="facts">
        <Fact label="Created" value={formatDate(resource.created_at)} />
        <Fact label="Updated" value={formatDate(resource.updated_at)} />
        <Fact label="Deleted" value={resource.deleted_at ? formatDate(resource.deleted_at) : "-"} />
        <Fact label="Object" value={resource.content?.key ?? "-"} />
        <Fact label="Size" value={formatBytes(resource.content?.size ?? 0)} />
        <Fact label="MIME" value={resource.content?.mime_type ?? "-"} />
        <Fact label="Kind source" value={kindDefinition?.source ?? "-"} />
        <Fact label="Kind schema" value={kindDefinition?.schema_id ?? "-"} />
        <Fact label="Content kind" value={kindDefinition ? (kindDefinition.supports_content ? "yes" : "no") : "-"} />
        <Fact label="Kind actions" value={kindDefinition?.actions.map((action) => action.id).join(", ") || "-"} />
        <Fact
          label="Available actions"
          value={resource.actions.available_actions.map((action) => action.id).join(", ") || "-"}
        />
      </section>
    </div>
  );
}

function TextInput({
  label,
  value,
  onChange,
  list,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  list?: string;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <input value={value} list={list} onChange={(event) => onChange(event.target.value)} />
    </label>
  );
}

function SelectInput({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: ResourceKindOption[];
  onChange: (value: string) => void;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <select value={value} onChange={(event) => onChange(event.target.value)}>
        {options.map((option) => (
          <option key={option.kind} value={option.kind}>
            {kindOptionLabel(option)}
          </option>
        ))}
      </select>
      {value && <small>{kindOptionHint(options.find((option) => option.kind === value))}</small>}
    </label>
  );
}

function Fact({ label, value }: { label: string; value: string }) {
  return (
    <div className="fact">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function PluginActionResult({ output }: { output: PluginActionOutput }) {
  const body = output.body;
  const view = body.view || "json";

  if (view === "html") {
    const html = typeof body.html === "string" ? body.html : String(body.content ?? "");
    return <iframe className="plugin-html-frame" sandbox="allow-scripts" title={output.action} srcDoc={html} />;
  }

  if (view === "binary_url") {
    const url = typeof body.url === "string" ? body.url : "";
    return (
      <div className="plugin-result">
        {url ? (
          <a className="primary-button" href={url} target="_blank" rel="noreferrer">
            Open
          </a>
        ) : (
          <pre>{JSON.stringify(body, null, 2)}</pre>
        )}
      </div>
    );
  }

  if (view === "text" || view === "markdown") {
    const text =
      typeof body.text === "string"
        ? body.text
        : typeof body.markdown === "string"
          ? body.markdown
          : String(body.content ?? "");
    return (
      <article className={view === "markdown" ? "plugin-markdown" : "reader-content"}>
        {text}
      </article>
    );
  }

  return (
    <div className="plugin-result">
      <pre>{JSON.stringify(body.content ?? body, null, 2)}</pre>
    </div>
  );
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${apiBase}${path}`, init);
  const text = await response.text();
  const data = text ? JSON.parse(text) : null;

  if (!response.ok) {
    throw new Error(data?.error ?? `${response.status} ${response.statusText}`);
  }

  return data as T;
}

function toDraft(resource: Resource): Draft {
  return {
    name: resource.name,
    kind: resource.kind,
    status: resource.status,
    description: resource.metadata.summary.description ?? "",
    tags: resource.metadata.summary.tags.join(", "),
    schemaId: resource.metadata.kind?.schema_id ?? "",
    kindData: JSON.stringify(resource.metadata.kind?.data ?? {}, null, 2),
  };
}

function metadataFromDraft(draft: Draft) {
  return buildMetadata(draft.description, draft.tags, draft.schemaId, draft.kindData);
}

function metadataFromUpload(draft: UploadDraft) {
  return buildMetadata(draft.description, draft.tags, draft.schemaId, draft.kindData);
}

function emptyCreateDraft(): Draft {
  return {
    name: "",
    kind: "core:unknown",
    status: "active",
    description: "",
    tags: "",
    schemaId: "",
    kindData: "{}",
  };
}

function emptyUploadDraft(): UploadDraft {
  return {
    file: null,
    name: "",
    kind: "core:file",
    directory: "uploads",
    description: "",
    tags: "",
    schemaId: "",
    kindData: "{}",
  };
}

function normalizeDraftKind<T extends { kind: string; schemaId: string }>(
  draft: T,
  options: ResourceKindOption[],
): T {
  if (options.some((option) => option.kind === draft.kind)) {
    return withKindDefaults(draft, options);
  }

  return withKindDefaults({ ...draft, kind: options[0]?.kind ?? draft.kind }, options);
}

function withKindDefaults<T extends { kind: string; schemaId: string }>(
  draft: T,
  options: ResourceKindOption[],
): T {
  const option = options.find((item) => item.kind === draft.kind);
  if (!option) return draft;

  return {
    ...draft,
    schemaId: option.schema_id ?? "",
  };
}

function kindOptionLabel(option: ResourceKindOption): string {
  return `${option.label} (${option.kind})`;
}

function kindOptionHint(option: ResourceKindOption | undefined): string {
  if (!option) return "";
  const content = option.supports_content ? "content" : "metadata only";
  const actions = option.actions.length ? ` / ${option.actions.map((action) => action.id).join(", ")}` : "";
  const detect = option.detect
    ? ` / detects ${[...option.detect.mime_types, ...option.detect.extensions].join(", ")}`
    : "";
  return `${option.source} / ${content}${option.schema_id ? ` / ${option.schema_id}` : ""}${detect}${actions}`;
}

function isImageResource(resource: Resource): boolean {
  const mimeType = resource.content?.mime_type;
  return Boolean(mimeType && mimeType.startsWith("image/"));
}

function isPluginUiAction(action: ResourceActionDefinition): boolean {
  return !["download_content", "read", "view_inline", "preview", "thumbnail"].includes(action.id);
}

function detectKindForFile(file: File, options: ResourceKindOption[]): string {
  const mimeType = file.type.toLowerCase();
  const filename = file.name.toLowerCase();
  const matched = options
    .filter((option) => option.supports_content && option.detect)
    .map((option) => ({ option, score: detectScore(option, mimeType, filename) }))
    .filter((match) => match.score > 0)
    .sort((left, right) => right.score - left.score)[0];

  return matched?.option.kind ?? fallbackUploadKind(options);
}

function detectScore(option: ResourceKindOption, mimeType: string, filename: string): number {
  const detect = option.detect;
  if (!detect) return 0;

  let score = 0;
  for (const mimeTypeRule of detect.mime_types) {
    if (mimeTypeRule.trim().endsWith("/*") && mimeMatches(mimeTypeRule, mimeType)) {
      score = Math.max(score, 10);
    } else if (mimeMatches(mimeTypeRule, mimeType)) {
      score = Math.max(score, 20);
    }
  }
  for (const extension of detect.extensions) {
    if (filename.endsWith(normalizeExtension(extension))) {
      score = Math.max(score, 30);
    }
  }

  return score;
}

function fallbackUploadKind(options: ResourceKindOption[]): string {
  return options.find((option) => option.kind === "core:file")?.kind ?? options[0]?.kind ?? "core:file";
}

function mimeMatches(rule: string, mimeType: string): boolean {
  const normalizedRule = rule.trim().toLowerCase();
  if (!normalizedRule || !mimeType) return false;
  if (normalizedRule === mimeType) return true;
  const prefix = normalizedRule.endsWith("/*") ? normalizedRule.slice(0, -1) : "";
  return Boolean(prefix && mimeType.startsWith(prefix));
}

function normalizeExtension(value: string): string {
  const extension = value.trim().toLowerCase();
  if (!extension) return extension;
  return extension.startsWith(".") ? extension : `.${extension}`;
}

function buildResourceTreeRows(resources: Resource[]): ResourceTreeRow[] {
  const sorted = [...resources].sort((left, right) => resourceSortPath(left).localeCompare(resourceSortPath(right)));
  const folderCounts = new Map<string, number>();

  for (const resource of sorted) {
    const segments = resourceDirectorySegments(resource);
    for (let index = 0; index < segments.length; index += 1) {
      const path = segments.slice(0, index + 1).join("/");
      folderCounts.set(path, (folderCounts.get(path) ?? 0) + 1);
    }
  }

  const rows: ResourceTreeRow[] = [];
  const emittedFolders = new Set<string>();
  for (const resource of sorted) {
    const segments = resourceDirectorySegments(resource);
    for (let index = 0; index < segments.length; index += 1) {
      const path = segments.slice(0, index + 1).join("/");
      if (emittedFolders.has(path)) continue;
      emittedFolders.add(path);
      rows.push({
        type: "folder",
        path,
        name: segments[index],
        depth: index,
        count: folderCounts.get(path) ?? 0,
      });
    }

    rows.push({
      type: "resource",
      resource,
      depth: segments.length,
    });
  }

  return rows;
}

function directoriesFromResources(resources: Resource[]): string[] {
  const directories = new Set<string>(["uploads"]);
  for (const resource of resources) {
    const segments = resourceDirectorySegments(resource);
    for (let index = 0; index < segments.length; index += 1) {
      directories.add(segments.slice(0, index + 1).join("/"));
    }
  }

  return [...directories].sort((left, right) => left.localeCompare(right));
}

function resourceDirectorySegments(resource: Resource): string[] {
  const key = resource.content?.key;
  if (!key || !key.includes("/")) return [];

  return key.split("/").slice(0, -1).filter(Boolean);
}

function resourceSortPath(resource: Resource): string {
  return resource.content?.key ?? `~metadata/${resource.name}`;
}

function normalizeDirectoryInput(value: string): string {
  return value
    .trim()
    .replaceAll("\\", "/")
    .split("/")
    .map((part) => part.trim())
    .filter((part) => part && part !== ".")
    .join("/");
}

function buildMetadata(description: string, tags: string, schemaId: string, kindData: string) {
  const trimmedSchemaId = schemaId.trim();
  return {
    summary: {
      description: description.trim() || null,
      tags: splitTags(tags),
    },
    kind: trimmedSchemaId
      ? {
          schema_id: trimmedSchemaId,
          data: parseKindData(kindData),
        }
      : null,
  };
}

function splitTags(value: string): string[] {
  return value
    .split(",")
    .map((tag) => tag.trim())
    .filter(Boolean);
}

function parseKindData(value: string): Record<string, unknown> {
  const trimmed = value.trim();
  if (!trimmed) return {};

  const parsed = JSON.parse(trimmed);
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error("Kind data must be a JSON object");
  }

  return parsed as Record<string, unknown>;
}

async function sha256Hex(file: File): Promise<string> {
  const buffer = await file.arrayBuffer();
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function formatBytes(value: number): string {
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  const amount = value / 1024 ** index;
  return `${amount.toFixed(index === 0 ? 0 : 1)} ${units[index]}`;
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : "Request failed";
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

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
  MoreHorizontal,
  Plus,
  RefreshCcw,
  Search,
  SlidersHorizontal,
  Users,
} from "lucide-react";
import type {
  CurrentUser,
  DirectoryAccessEntry,
  Resource,
  ResourceActionDefinition,
  ResourceDirectory,
  ResourceKindOption,
  ResourcePage,
} from "../../api/contracts";
import { cx, iconButtonClass, inputClass, primaryButtonClass, secondaryButtonClass } from "../../components/ui";
import {
  actionLocations,
  actionsAt,
  executeResourceAction,
  selectThumbnailAction,
} from "../../plugins/host/actions";
import { assetHubUrl, mediaSource } from "../../plugins/views";
import { formatBytes, formatDate, kindOptionLabel } from "../../utils/resourceDrafts";
import { shouldCloseAfterCreate } from "./dialogBehavior";
import { directoryBreadcrumbs, parentDirectoryWithinRoot } from "./directoryNavigation";
import type { Filters } from "./models";

type ResourceBrowserProps = {
  user: CurrentUser;
  page: ResourcePage;
  folders: ResourceDirectory[];
  filters: Filters;
  directoryEntries: DirectoryAccessEntry[];
  activeEntryDirectory: string;
  onDirectoryEntryChange: (path: string) => void;
  updateFilters: (patch: Partial<Filters>) => void;
  setPage: (page: number) => void;
  currentDirectory: string;
  openDirectory: (path: string) => void;
  kinds: ResourceKindOption[];
  selected: Resource | null;
  select: (resource: Resource) => void;
  loading: boolean;
  scanPending: boolean;
  folderPending: boolean;
  error: string | null;
  notice: string | null;
  clearNotice: () => void;
  reload: () => void;
  onScan: () => void;
  onUsers: () => void;
  onCreate: () => void;
  onUpload: () => void;
  onCreateFolder: (name: string) => Promise<unknown>;
  onAction: (resource: Resource, action: ResourceActionDefinition) => void;
  onLogout: () => Promise<void>;
};

export function ResourceBrowser(props: ResourceBrowserProps) {
  const [folderOpen, setFolderOpen] = React.useState(false);
  const [folderName, setFolderName] = React.useState("");
  const totalPages = Math.max(1, Math.ceil(props.page.total / props.page.limit));
  const activeEntry = props.directoryEntries.find(
    (entry) => entry.directory === props.activeEntryDirectory,
  );
  const navigationRoot = props.user.is_admin ? "" : props.activeEntryDirectory;
  const breadcrumbs = directoryBreadcrumbs(
    props.currentDirectory,
    navigationRoot,
    props.user.is_admin ? "Root" : activeEntry?.is_workspace
      ? "Workspace"
      : navigationRoot || "Root",
  );
  const parentDirectory = parentDirectoryWithinRoot(props.currentDirectory, navigationRoot);

  return (
    <section className="flex min-w-0 flex-col border-r border-slate-200 bg-white" aria-label="Resources">
      <header className="flex min-h-20 items-center justify-between gap-4 border-b border-slate-200 px-6 py-4 max-md:flex-col max-md:items-stretch">
        <div className="flex items-center gap-3">
          <div className="flex size-10 items-center justify-center rounded-xl bg-blue-600 text-white"><Database size={20} /></div>
          <div><h1 className="text-lg font-bold">Asset Hub</h1><span className="text-xs text-slate-500">{props.page.total} resources</span></div>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          <button className={iconButtonClass} onClick={props.reload} title="Refresh"><RefreshCcw className={props.loading ? "animate-spin" : ""} size={18} /></button>
          {props.user.is_admin && <button className={secondaryButtonClass} onClick={props.onScan} disabled={props.scanPending}><Database size={18} />Scan</button>}
          {props.user.is_admin && <button className={secondaryButtonClass} onClick={props.onUsers}><Users size={18} />Users</button>}
          <button className={secondaryButtonClass} onClick={() => { setFolderName(""); setFolderOpen(true); }}><FolderPlus size={18} />New folder</button>
          <button className={secondaryButtonClass} onClick={props.onCreate}><Plus size={18} />New</button>
          <button className={primaryButtonClass} onClick={props.onUpload}><FileUp size={18} />Upload</button>
          <button className={iconButtonClass} onClick={() => void props.onLogout()} title="Sign out"><LogOut size={18} /></button>
        </div>
      </header>

      <div className="grid gap-3 border-b border-slate-200 bg-slate-50 p-4 sm:grid-cols-2 xl:grid-cols-[minmax(220px,1fr)_minmax(180px,240px)_minmax(120px,180px)_auto_auto]">
        <label className="flex h-10 items-center gap-2 rounded-lg border bg-white px-3"><Search size={17} /><input className="w-full outline-none" value={props.filters.q} onChange={(event) => props.updateFilters({ q: event.target.value })} placeholder="Search name" /></label>
        <label className="flex h-10 items-center gap-2 rounded-lg border bg-white px-3"><SlidersHorizontal size={16} /><select className="w-full outline-none" value={props.filters.kind} onChange={(event) => props.updateFilters({ kind: event.target.value })}><option value="">All kinds</option>{props.kinds.map((kind) => <option key={kind.kind} value={kind.kind}>{kindOptionLabel(kind)}</option>)}</select></label>
        <input className={cx(inputClass, "h-10")} value={props.filters.tag} onChange={(event) => props.updateFilters({ tag: event.target.value })} placeholder="tag" />
        <label className="flex items-center gap-2"><input type="checkbox" checked={props.filters.includeDeleted} onChange={(event) => props.updateFilters({ includeDeleted: event.target.checked })} />Deleted</label>
        <label className="flex items-center gap-2"><input type="checkbox" checked={props.filters.includeDescendants} disabled={!props.filters.kind} onChange={(event) => props.updateFilters({ includeDescendants: event.target.checked })} />Descendants</label>
      </div>

      {props.error && <div className="mx-4 mt-3 rounded-lg bg-red-50 px-4 py-3 text-sm text-red-700">{props.error}</div>}
      {props.notice && <div className="mx-4 mt-3 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-700" onAnimationEnd={props.clearNotice}>{props.notice}</div>}

      <nav className="flex min-h-10 flex-wrap items-center gap-2 border-b px-6 py-2 text-sm">
        {!props.user.is_admin && props.directoryEntries.length > 1 && (
          <select aria-label="Directory entry" className={cx(inputClass, "h-8 min-w-0 max-w-72 py-1")} value={props.activeEntryDirectory} onChange={(event) => props.onDirectoryEntryChange(event.target.value)}>
            {props.directoryEntries.map((entry) => <option key={entry.directory} value={entry.directory}>{entry.is_workspace ? "Workspace: " : ""}{entry.directory || "/"} ({entry.permission})</option>)}
          </select>
        )}
        <div className="flex min-w-0 items-center gap-1 overflow-hidden">
          {breadcrumbs.map((crumb, index) => <React.Fragment key={crumb.path || "root"}>{index > 0 && <ChevronRight className="shrink-0" size={14} />}<button className="truncate text-blue-600" onClick={() => props.openDirectory(crumb.path)}>{crumb.label}</button></React.Fragment>)}
        </div>
      </nav>

      <div className="flex min-h-0 flex-1 flex-col overflow-auto">
        {parentDirectory !== null && <button className="grid min-h-11 grid-cols-[1.5rem_1fr] items-center gap-2 border-b bg-slate-50 px-6 text-left" onClick={() => props.openDirectory(parentDirectory)}><Folder size={18} /><span>..</span></button>}
        {props.folders.map((folder) => <button key={folder.path} className="grid min-h-11 grid-cols-[1.5rem_1fr] items-center gap-2 border-b bg-slate-50 px-6 text-left" onClick={() => props.openDirectory(folder.path)}><Folder size={18} /><span>{folder.name}</span></button>)}
        {props.page.items.map((resource) => (
          <ResourceRow
            key={resource.id}
            resource={resource}
            selected={props.selected?.id === resource.id}
            onSelect={() => props.select(resource)}
            onAction={(action) => props.onAction(resource, action)}
          />
        ))}
        {!props.loading && !props.folders.length && !props.page.items.length && <div className="grid min-h-48 place-items-center text-sm">No resources</div>}
      </div>

      <footer className="flex min-h-16 items-center justify-end gap-3 border-t px-6">
        <button className={iconButtonClass} disabled={props.filters.page <= 1} onClick={() => props.setPage(props.filters.page - 1)}><ChevronLeft size={18} /></button>
        <span>{props.filters.page} / {totalPages}</span>
        <button className={iconButtonClass} disabled={props.filters.page >= totalPages} onClick={() => props.setPage(props.filters.page + 1)}><ChevronRight size={18} /></button>
      </footer>

      {folderOpen && (
        <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/50">
          <form className="grid w-full max-w-md gap-4 rounded-lg bg-white p-6" onSubmit={(event) => { event.preventDefault(); void props.onCreateFolder(folderName).then((created) => { if (shouldCloseAfterCreate(created)) setFolderOpen(false); }); }}>
            <h2 className="text-xl font-bold">New folder</h2>
            <input className={inputClass} value={folderName} onChange={(event) => setFolderName(event.target.value)} />
            <div className="flex justify-end gap-2"><button type="button" onClick={() => setFolderOpen(false)}>Cancel</button><button className={primaryButtonClass} disabled={props.folderPending || !folderName.trim()}>Create</button></div>
          </form>
        </div>
      )}
    </section>
  );
}

function ResourceRow({ resource, selected, onSelect, onAction }: {
  resource: Resource;
  selected: boolean;
  onSelect: () => void;
  onAction: (action: ResourceActionDefinition) => void;
}) {
  const contextActions = actionsAt(resource, actionLocations.contextMenu);
  const [menuOpen, setMenuOpen] = React.useState(false);
  return (
    <div className={cx("relative grid min-h-20 grid-cols-[minmax(0,1fr)_auto] items-center border-b hover:bg-blue-50", selected && "bg-blue-50")}>
      <button className="grid min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-4 px-6 py-3 text-left" onClick={onSelect}>
        <ResourceThumbnail resource={resource} />
        <div className="min-w-0"><div className="truncate font-semibold">{resource.name}</div><div className="flex gap-3 text-xs text-slate-500"><span>{resource.kind}</span><span>{formatBytes(resource.content?.size ?? 0)}</span><span>{formatDate(resource.updated_at)}</span></div></div>
      </button>
      <div className="flex items-center gap-2 pr-4">
        <span className="text-xs">{resource.status}</span>
        {contextActions.length > 0 && (
          <div className="relative">
            <button className={iconButtonClass} type="button" title="Resource actions" onClick={() => setMenuOpen((open) => !open)}><MoreHorizontal size={18} /></button>
            {menuOpen && <div className="absolute right-0 z-20 mt-1 min-w-48 overflow-hidden rounded-lg border border-slate-200 bg-white py-1 shadow-xl">{contextActions.map((action) => <button className="block w-full px-3 py-2 text-left text-sm hover:bg-slate-50" key={action.id} type="button" onClick={() => { setMenuOpen(false); onAction(action); }}>{action.label}</button>)}</div>}
          </div>
        )}
      </div>
    </div>
  );
}

function ResourceThumbnail({ resource }: { resource: Resource }) {
  const action = selectThumbnailAction(resource);
  const [source, setSource] = React.useState<string | null>(null);
  const [loading, setLoading] = React.useState(Boolean(action));

  React.useEffect(() => {
    let active = true;
    setSource(null);
    setLoading(Boolean(action));
    if (!action) return () => { active = false; };
    void executeResourceAction(resource, action.id)
      .then((output) => {
        if (!active) return;
        if (output.view.view === "media" && output.view.mime_type.startsWith("image/")) {
          setSource(mediaSource(output.view));
        } else if (output.view.view === "binary_url" && output.view.mime_type?.startsWith("image/")) {
          setSource(assetHubUrl(output.view.url));
        }
      })
      .catch(() => undefined)
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [action?.id, resource.id, resource.updated_at]);

  if (source) return <img className="size-14 rounded-lg object-cover" src={source} alt={`${resource.name} cover`} onError={() => setSource(null)} />;
  return <div className="flex size-14 items-center justify-center rounded-lg border border-dashed">{loading ? <Loader2 className="animate-spin text-slate-400" size={18} /> : <FileIcon size={18} />}</div>;
}

import React from "react";
import { ChevronLeft, ChevronRight, Database, File as FileIcon, FileUp, Folder, FolderPlus, Loader2, LogOut, Plus, RefreshCcw, Search, SlidersHorizontal, Users } from "lucide-react";
import { apiBase } from "../../api";
import { cx, iconButtonClass, inputClass, primaryButtonClass, secondaryButtonClass } from "../../components/ui";
import type { CurrentUser } from "../../components/AuthGate";
import type { DirectoryAccessEntry, Filters, Resource, ResourceDirectory, ResourceKindOption, ResourcePage } from "../../types";
import { formatBytes, formatDate, hasAction, kindOptionLabel } from "../../utils/resourceDrafts";
import { shouldCloseAfterCreate } from "./dialogBehavior.js";
import { directoryBreadcrumbs, parentDirectoryWithinRoot } from "./directoryNavigation.js";

export function ResourceBrowser(props: {
  user: CurrentUser; page: ResourcePage; folders: ResourceDirectory[]; filters: Filters;
  directoryEntries: DirectoryAccessEntry[]; activeEntryDirectory: string;
  onDirectoryEntryChange: (path: string) => void;
  updateFilters: (patch: Partial<Filters>) => void; setPage: (page: number) => void;
  currentDirectory: string; openDirectory: (path: string) => void; kinds: ResourceKindOption[];
  selected: Resource | null; select: (resource: Resource) => void; loading: boolean;
  scanPending: boolean; folderPending: boolean;
  error: string | null; notice: string | null; clearNotice: () => void; reload: () => void;
  onScan: () => void; onUsers: () => void; onCreate: () => void; onUpload: () => void;
  onCreateFolder: (name: string) => Promise<unknown>; onLogout: () => Promise<void>;
}) {
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

  return <section className="flex min-w-0 flex-col border-r border-slate-200 bg-white" aria-label="Resources">
    <header className="flex min-h-20 items-center justify-between gap-4 border-b border-slate-200 px-6 py-4 max-md:flex-col max-md:items-stretch">
      <div className="flex items-center gap-3"><div className="flex size-10 items-center justify-center rounded-xl bg-blue-600 text-white"><Database size={20} /></div>
        <div><h1 className="text-lg font-bold">Asset Hub</h1><span className="text-xs text-slate-500">{props.page.total} resources</span></div></div>
      <div className="flex flex-wrap justify-end gap-2">
        <button className={iconButtonClass} onClick={props.reload}>{props.loading ? <Loader2 className="animate-spin" size={18} /> : <RefreshCcw size={18} />}</button>
        {props.user.is_admin && <button className={secondaryButtonClass} onClick={props.onScan} disabled={props.scanPending}><Database size={18} />Scan</button>}
        {props.user.is_admin && <button className={secondaryButtonClass} onClick={props.onUsers}><Users size={18} />Users</button>}
        <button className={secondaryButtonClass} onClick={() => { setFolderName(""); setFolderOpen(true); }}><FolderPlus size={18} />New folder</button>
        <button className={secondaryButtonClass} onClick={props.onCreate}><Plus size={18} />New</button>
        <button className={primaryButtonClass} onClick={props.onUpload}><FileUp size={18} />Upload</button>
        <button className={iconButtonClass} onClick={() => void props.onLogout()}><LogOut size={18} /></button>
      </div>
    </header>
    <div className="grid gap-3 border-b border-slate-200 bg-slate-50 p-4 sm:grid-cols-2 xl:grid-cols-[minmax(220px,1fr)_minmax(180px,240px)_minmax(120px,180px)_auto_auto]">
      <label className="flex h-10 items-center gap-2 rounded-lg border bg-white px-3"><Search size={17} /><input className="w-full outline-none" value={props.filters.q} onChange={(e) => props.updateFilters({ q: e.target.value })} placeholder="Search name" /></label>
      <label className="flex h-10 items-center gap-2 rounded-lg border bg-white px-3"><SlidersHorizontal size={16} /><select className="w-full outline-none" value={props.filters.kind} onChange={(e) => props.updateFilters({ kind: e.target.value })}><option value="">All kinds</option>{props.kinds.map((kind) => <option key={kind.kind} value={kind.kind}>{kindOptionLabel(kind)}</option>)}</select></label>
      <input className={cx(inputClass, "h-10")} value={props.filters.tag} onChange={(e) => props.updateFilters({ tag: e.target.value })} placeholder="tag" />
      <label className="flex items-center gap-2"><input type="checkbox" checked={props.filters.includeDeleted} onChange={(e) => props.updateFilters({ includeDeleted: e.target.checked })} />Deleted</label>
      <label className="flex items-center gap-2"><input type="checkbox" checked={props.filters.includeDescendants} disabled={!props.filters.kind} onChange={(e) => props.updateFilters({ includeDescendants: e.target.checked })} />Descendants</label>
    </div>
    {props.error && <div className="mx-4 mt-3 rounded-lg bg-red-50 px-4 py-3 text-sm text-red-700">{props.error}</div>}
    {props.notice && <div className="mx-4 mt-3 rounded-lg bg-emerald-50 px-4 py-3 text-sm text-emerald-700" onAnimationEnd={props.clearNotice}>{props.notice}</div>}
    <nav className="flex min-h-10 flex-wrap items-center gap-2 border-b px-6 py-2 text-sm">
      {!props.user.is_admin && props.directoryEntries.length > 1 && <select
        aria-label="Directory entry"
        className={cx(inputClass, "h-8 min-w-0 max-w-72 py-1")}
        value={props.activeEntryDirectory}
        onChange={(event) => props.onDirectoryEntryChange(event.target.value)}
      >
        {props.directoryEntries.map((entry) => <option key={entry.directory} value={entry.directory}>
          {entry.is_workspace ? "Workspace: " : ""}{entry.directory || "/"} ({entry.permission})
        </option>)}
      </select>}
      <div className="flex min-w-0 items-center gap-1 overflow-hidden">{breadcrumbs.map((crumb, index) => <React.Fragment key={crumb.path || "root"}>{index > 0 && <ChevronRight className="shrink-0" size={14} />}<button className="truncate text-blue-600" onClick={() => props.openDirectory(crumb.path)}>{crumb.label}</button></React.Fragment>)}</div>
    </nav>
    <div className="flex min-h-0 flex-1 flex-col overflow-auto">
      {parentDirectory !== null && <button className="grid min-h-11 grid-cols-[1.5rem_1fr] items-center gap-2 border-b bg-slate-50 px-6 text-left" onClick={() => props.openDirectory(parentDirectory)}><Folder size={18} /><span>..</span></button>}
      {props.folders.map((folder) => <button key={folder.path} className="grid min-h-11 grid-cols-[1.5rem_1fr] items-center gap-2 border-b bg-slate-50 px-6 text-left" onClick={() => props.openDirectory(folder.path)}><Folder size={18} /><span>{folder.name}</span></button>)}
      {props.page.items.map((resource) => <button key={resource.id} className={cx("grid min-h-20 grid-cols-[3.5rem_minmax(0,1fr)_auto] items-center gap-4 border-b px-6 py-3 text-left hover:bg-blue-50", props.selected?.id === resource.id && "bg-blue-50")} onClick={() => props.select(resource)}>
        {hasAction(resource, "thumbnail") ? <img className="size-14 rounded-lg object-cover" src={`${apiBase}/resources/${resource.id}/thumbnail`} alt="" /> : <div className="flex size-14 items-center justify-center rounded-lg border border-dashed"><FileIcon size={18} /></div>}
        <div className="min-w-0"><div className="truncate font-semibold">{resource.name}</div><div className="flex gap-3 text-xs text-slate-500"><span>{resource.kind}</span><span>{formatBytes(resource.content?.size ?? 0)}</span><span>{formatDate(resource.updated_at)}</span></div></div>
        <span className="text-xs">{resource.status}</span>
      </button>)}
      {!props.loading && !props.folders.length && !props.page.items.length && <div className="grid min-h-48 place-items-center text-sm">No resources</div>}
    </div>
    <footer className="flex min-h-16 items-center justify-end gap-3 border-t px-6"><button className={iconButtonClass} disabled={props.filters.page <= 1} onClick={() => props.setPage(props.filters.page - 1)}><ChevronLeft size={18} /></button><span>{props.filters.page} / {totalPages}</span><button className={iconButtonClass} disabled={props.filters.page >= totalPages} onClick={() => props.setPage(props.filters.page + 1)}><ChevronRight size={18} /></button></footer>
    {folderOpen && <div className="fixed inset-0 z-50 grid place-items-center bg-slate-950/50"><form className="grid w-full max-w-md gap-4 rounded-2xl bg-white p-6" onSubmit={(e) => { e.preventDefault(); void props.onCreateFolder(folderName).then((created) => { if (shouldCloseAfterCreate(created)) setFolderOpen(false); }); }}><h2 className="text-xl font-bold">New folder</h2><input className={inputClass} value={folderName} onChange={(e) => setFolderName(e.target.value)} /><div className="flex justify-end gap-2"><button type="button" onClick={() => setFolderOpen(false)}>Cancel</button><button className={primaryButtonClass} disabled={props.folderPending || !folderName.trim()}>Create</button></div></form></div>}
  </section>;
}

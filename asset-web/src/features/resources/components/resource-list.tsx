import { useQuery } from "@tanstack/react-query";
import {
  ChevronLeft,
  ChevronRight,
  Database,
  File,
  FileUp,
  Folder,
  FolderPlus,
  LogOut,
  Plus,
  RefreshCw,
  Search,
  Users,
} from "lucide-react";
import React from "react";
import { useGateway } from "@/application/ports/gateway-context";
import type { CurrentUser } from "@/domain/auth";
import { breadcrumbs, parentDirectory } from "@/domain/directory-path";
import type {
  Directory,
  DirectoryAction,
  DirectoryListing,
  Resource,
  ResourceAction,
  ResourceFilters,
  ResourceKind,
} from "@/domain/resource";
import { formatBytes, formatDate } from "@/domain/resource-draft";
import { usePluginKernel } from "@/kernel/plugin-kernel";
import { hostSlots } from "@/kernel/slots";
import { Button } from "@/shared/ui/button";
import { ActionMenu, ActionMenuItem } from "@/shared/ui/dropdown";
import { controlClass, Input } from "@/shared/ui/field";
import { ErrorState, LoadingState } from "@/shared/ui/state";

export function ResourceList({
  user,
  listing,
  kinds,
  filters,
  selectedId,
  selectedDirectoryId,
  loading,
  error,
  onFilters,
  onOpenDirectory,
  onSelect,
  onSelectDirectory,
  onAction,
  onDirectoryAction,
  onRefresh,
  onCreate,
  onUpload,
  onCreateFolder,
  onUsers,
  onLogout,
}: {
  user: Pick<CurrentUser, "username" | "isAdmin">;
  listing: DirectoryListing | undefined;
  kinds: ResourceKind[];
  filters: ResourceFilters;
  selectedId: string | null;
  selectedDirectoryId: string | null;
  loading: boolean;
  error: unknown;
  onFilters: (patch: Partial<ResourceFilters>) => void;
  onOpenDirectory: (path: string) => void;
  onSelect: (resource: Resource) => void;
  onSelectDirectory: (directory: Directory) => void;
  onAction: (resource: Resource, action: ResourceAction) => void;
  onDirectoryAction: (directory: Directory, action: DirectoryAction) => void;
  onRefresh: () => void;
  onCreate: () => void;
  onUpload: () => void;
  onCreateFolder: () => void;
  onUsers: () => void;
  onLogout: () => void;
}) {
  const total = listing?.resources.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / filters.limit));
  const crumbs = breadcrumbs(filters.directory);
  const parent = parentDirectory(filters.directory);
  const kernel = usePluginKernel();
  const currentDirectory = listing?.directory;
  const directoryActions = currentDirectory
    ? kernel.directoryActionsAt(currentDirectory, hostSlots.directoryToolbar)
    : [];

  return (
    <section className="flex min-h-0 min-w-0 flex-col bg-white" aria-label="Resource workspace">
      <header className="flex min-h-20 flex-wrap items-center justify-between gap-4 border-b border-slate-200 px-5 py-4 xl:px-7">
        <div className="flex items-center gap-3">
          <span className="grid size-11 place-items-center rounded-2xl bg-blue-600 text-white">
            <Database size={21} />
          </span>
          <div>
            <h1 className="font-bold text-slate-950">Asset Hub</h1>
            <p className="text-xs text-slate-500">
              {total} resources · {user.username}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="ghost" size="icon" aria-label="Refresh" onClick={onRefresh}>
            <RefreshCw className={loading ? "animate-spin" : ""} size={18} />
          </Button>
          {user.isAdmin ? (
            <Button variant="secondary" size="small" onClick={onUsers}>
              <Users size={16} />
              Users
            </Button>
          ) : null}
          <Button variant="secondary" size="small" onClick={onCreateFolder}>
            <FolderPlus size={16} />
            Folder
          </Button>
          <Button variant="secondary" size="small" onClick={onCreate}>
            <Plus size={16} />
            Resource
          </Button>
          <Button size="small" onClick={onUpload}>
            <FileUp size={16} />
            Upload
          </Button>
          <Button variant="ghost" size="icon" aria-label="Sign out" onClick={onLogout}>
            <LogOut size={18} />
          </Button>
        </div>
      </header>

      <div className="grid gap-3 border-b border-slate-200 bg-slate-50/80 p-4 md:grid-cols-2 xl:grid-cols-[minmax(180px,1fr)_minmax(160px,220px)_minmax(120px,180px)_auto_auto]">
        <div className="relative">
          <Search className="absolute left-3 top-3 text-slate-400" size={16} />
          <Input
            className="pl-9"
            aria-label="Search resources"
            placeholder="Search resources"
            value={filters.query}
            onChange={(event) => onFilters({ query: event.target.value, page: 1 })}
          />
        </div>
        <select
          aria-label="Resource kind"
          className={controlClass}
          value={filters.kind}
          onChange={(event) => onFilters({ kind: event.target.value, page: 1 })}
        >
          <option value="">All kinds</option>
          {kinds.map((kind) => (
            <option key={kind.kind} value={kind.kind}>
              {kind.label}
            </option>
          ))}
        </select>
        <Input
          aria-label="Tag"
          placeholder="Filter by tag"
          value={filters.tag}
          onChange={(event) => onFilters({ tag: event.target.value, page: 1 })}
        />
        <Toggle
          label="Deleted"
          checked={filters.includeDeleted}
          onChange={(includeDeleted) => onFilters({ includeDeleted, page: 1 })}
        />
        <Toggle
          label="Descendants"
          checked={filters.includeDescendants}
          disabled={!filters.kind}
          onChange={(includeDescendants) => onFilters({ includeDescendants, page: 1 })}
        />
      </div>

      <nav className="flex min-h-12 flex-wrap items-center gap-3 border-b border-slate-200 px-5 py-2 text-sm xl:px-7">
        <div className="flex min-w-0 items-center gap-1 overflow-hidden">
          {crumbs.map((crumb, index) => (
            <React.Fragment key={crumb.path || "root"}>
              {index ? <ChevronRight className="shrink-0 text-slate-300" size={15} /> : null}
              <button
                className="max-w-48 truncate font-medium text-blue-700 hover:underline"
                type="button"
                onClick={() => onOpenDirectory(crumb.path)}
              >
                {crumb.label}
              </button>
            </React.Fragment>
          ))}
        </div>
        {directoryActions.length ? (
          <div className="ml-auto flex flex-wrap gap-2">
            {directoryActions.map((action) => (
              <Button
                key={action.id}
                variant="secondary"
                size="small"
                onClick={() => {
                  if (currentDirectory) onDirectoryAction(currentDirectory, action);
                }}
              >
                {action.label}
              </Button>
            ))}
          </div>
        ) : null}
      </nav>

      <div className="min-h-0 flex-1 overflow-auto">
        {error ? <ErrorState error={error} /> : null}
        {loading && !listing ? <LoadingState label="Loading resources" /> : null}
        {parent !== null ? <FolderRow name=".." onClick={() => onOpenDirectory(parent)} /> : null}
        {listing?.folders.map((folder) => (
          <FolderRow
            key={folder.id}
            name={folder.name}
            directory={folder}
            selected={folder.id === selectedDirectoryId}
            onSelect={() => onSelectDirectory(folder)}
            onOpen={() => onOpenDirectory(folder.path)}
            onAction={(action) => onDirectoryAction(folder, action)}
          />
        ))}
        {listing?.resources.items.map((resource) => (
          <ResourceRow
            key={resource.id}
            resource={resource}
            selected={resource.id === selectedId}
            onSelect={() => onSelect(resource)}
            onAction={(action) => onAction(resource, action)}
          />
        ))}
        {!loading && listing && !listing.folders.length && !listing.resources.items.length ? (
          <div className="grid min-h-52 place-items-center text-sm text-slate-400">
            This folder is empty
          </div>
        ) : null}
      </div>

      <footer className="flex min-h-16 items-center justify-between border-t border-slate-200 px-5 text-sm text-slate-500 xl:px-7">
        <span>
          Page {filters.page} of {totalPages}
        </span>
        <div className="flex gap-1">
          <Button
            variant="ghost"
            size="icon"
            disabled={filters.page <= 1}
            onClick={() => onFilters({ page: filters.page - 1 })}
          >
            <ChevronLeft size={18} />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            disabled={filters.page >= totalPages}
            onClick={() => onFilters({ page: filters.page + 1 })}
          >
            <ChevronRight size={18} />
          </Button>
        </div>
      </footer>
    </section>
  );
}

function FolderRow({
  name,
  directory,
  selected = false,
  onClick,
  onSelect,
  onOpen,
  onAction,
}: {
  name: string;
  directory?: Directory;
  selected?: boolean;
  onClick?: () => void;
  onSelect?: () => void;
  onOpen?: () => void;
  onAction?: (action: DirectoryAction) => void;
}) {
  const kernel = usePluginKernel();
  const actions = directory
    ? kernel.directoryActionsAt(directory, hostSlots.directoryContextMenu)
    : [];
  return (
    <div
      className={`grid min-h-14 grid-cols-[minmax(0,1fr)_auto] items-center border-b border-slate-100 transition ${selected ? "bg-blue-50 ring-1 ring-inset ring-blue-200" : "bg-slate-50/60 hover:bg-blue-50"}`}
    >
      <button
        className="grid min-h-14 w-full grid-cols-[3.5rem_1fr] items-center gap-3 px-5 text-left xl:px-7"
        type="button"
        aria-pressed={directory ? selected : undefined}
        onClick={onSelect ?? onClick}
        onDoubleClick={onOpen}
        onKeyDown={(event) => {
          if (event.key === "Enter" && onOpen) {
            event.preventDefault();
            onOpen();
          }
        }}
      >
        <span className="grid size-10 place-items-center rounded-xl bg-amber-100 text-amber-700">
          <Folder size={19} />
        </span>
        <span className="font-medium text-slate-800">{name}</span>
      </button>
      {actions.length && onAction ? (
        <div className="pr-4">
          <ActionMenu>
            {actions.map((action) => (
              <ActionMenuItem key={action.id} onSelect={() => onAction(action)}>
                {action.label}
              </ActionMenuItem>
            ))}
          </ActionMenu>
        </div>
      ) : null}
    </div>
  );
}

function ResourceRow({
  resource,
  selected,
  onSelect,
  onAction,
}: {
  resource: Resource;
  selected: boolean;
  onSelect: () => void;
  onAction: (action: ResourceAction) => void;
}) {
  const kernel = usePluginKernel();
  const actions = kernel.actionsAt(resource, hostSlots.resourceContextMenu);
  return (
    <div
      className={`grid min-h-20 grid-cols-[minmax(0,1fr)_auto] items-center border-b border-slate-100 transition ${selected ? "bg-blue-50 ring-1 ring-inset ring-blue-200" : "hover:bg-slate-50"}`}
    >
      <button
        className="grid min-w-0 grid-cols-[3.5rem_minmax(0,1fr)] items-center gap-4 px-5 py-3 text-left xl:px-7"
        type="button"
        onClick={onSelect}
      >
        <ResourceThumbnail resource={resource} />
        <span className="min-w-0">
          <strong className="block truncate text-sm text-slate-900">{resource.name}</strong>
          <span className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-slate-500">
            <span>{resource.kind}</span>
            <span>{formatBytes(resource.content?.size ?? 0)}</span>
            <span>{formatDate(resource.updatedAt)}</span>
          </span>
        </span>
      </button>
      <div className="flex items-center gap-2 pr-4">
        {resource.deletedAt ? (
          <span className="rounded-full bg-red-50 px-2 py-1 text-[11px] font-semibold text-red-700">
            deleted
          </span>
        ) : null}
        {actions.length ? (
          <ActionMenu>
            {actions.map((action) => (
              <ActionMenuItem key={action.id} onSelect={() => onAction(action)}>
                {action.label}
              </ActionMenuItem>
            ))}
          </ActionMenu>
        ) : null}
      </div>
    </div>
  );
}

function ResourceThumbnail({ resource }: { resource: Resource }) {
  const gateway = useGateway();
  const kernel = usePluginKernel();
  const action = kernel.thumbnailAction(resource);
  const result = useQuery({
    queryKey: ["thumbnail", resource.id, resource.updatedAt, action?.id],
    queryFn: () => gateway.executeAction(resource, action?.id ?? ""),
    enabled: Boolean(action),
    retry: false,
    staleTime: 5 * 60_000,
  });
  const image = result.data
    ? thumbnailImage(result.data.view, gateway.assetUrl.bind(gateway))
    : null;
  if (image)
    return <img className="size-14 rounded-xl bg-slate-100 object-cover" src={image} alt="" />;
  return (
    <span className="grid size-14 place-items-center rounded-xl border border-dashed border-slate-300 bg-white text-slate-400">
      <File className={result.isPending ? "animate-pulse" : ""} size={19} />
    </span>
  );
}

function thumbnailImage(
  view: import("@/domain/plugin").PluginView,
  resolveUrl: (url: string) => string | null,
): string | null {
  if (view.view === "media" && view.mime_type.startsWith("image/")) {
    return view.encoding === "base64"
      ? `data:${view.mime_type};base64,${view.data}`
      : resolveUrl(view.data);
  }
  return null;
}

function Toggle({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex min-h-10 items-center gap-2 text-sm font-medium text-slate-600">
      <input
        className="size-4 accent-blue-600"
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      {label}
    </label>
  );
}

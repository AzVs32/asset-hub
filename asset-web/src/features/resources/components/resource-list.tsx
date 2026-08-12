import { useQuery } from "@tanstack/react-query";
import {
  ChevronLeft,
  ChevronRight,
  File,
  FileUp,
  Folder,
  FolderPlus,
  RefreshCw,
  Search,
} from "lucide-react";
import { useGateway } from "@/application/ports/gateway-context";
import { parentDirectory } from "@/domain/directory-path";
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
import { coreDirectoryWorkspaceSlots } from "@/kernel/slots";
import { Button } from "@/shared/ui/button";
import { ActionMenu, ActionMenuItem } from "@/shared/ui/dropdown";
import { Input } from "@/shared/ui/field";
import { ErrorState, LoadingState } from "@/shared/ui/state";
import { KindSelect } from "./kind-select";

export function ResourceList({
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
  onRestore,
  onDirectoryAction,
  onRefresh,
  onUpload,
  onCreateFolder,
}: {
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
  onRestore: (resource: Resource) => void;
  onDirectoryAction: (directory: Directory, action: DirectoryAction) => void;
  onRefresh: () => void;
  onUpload: () => void;
  onCreateFolder: () => void;
}) {
  const total = listing?.resources.total ?? 0;
  const totalPages = Math.max(1, Math.ceil(total / filters.limit));
  const parent = parentDirectory(filters.directory);

  return (
    <section
      className="flex min-h-0 min-w-0 flex-col overflow-hidden rounded-3xl border border-slate-200/80 bg-white shadow-[0_18px_50px_-30px_rgba(15,23,42,0.45)]"
      aria-label="Resource workspace"
    >
      <header className="flex min-h-[4.75rem] flex-wrap items-center justify-between gap-4 border-b border-slate-100 px-5 py-3.5 xl:px-6">
        <div>
          <p className="text-[10px] font-bold uppercase tracking-[0.16em] text-indigo-500">
            Directory
          </p>
          <h2 className="mt-0.5 text-lg font-bold tracking-[-0.025em] text-slate-950">
            {listing?.directory.name || "Root"}
          </h2>
          <p className="mt-0.5 text-xs font-medium text-slate-400">
            {listing?.folders.length ?? 0} folders · {total} assets
          </p>
        </div>
        <div className="flex flex-wrap justify-end gap-2">
          <Button variant="ghost" size="icon" aria-label="Refresh" onClick={onRefresh}>
            <RefreshCw className={loading ? "animate-spin" : ""} size={17} />
          </Button>
          <Button variant="secondary" size="small" onClick={onCreateFolder}>
            <FolderPlus size={15} />
            New folder
          </Button>
          <Button size="small" onClick={onUpload}>
            <FileUp size={15} />
            Upload
          </Button>
        </div>
      </header>

      <div className="grid gap-2.5 border-b border-slate-100 bg-slate-50/70 px-4 py-3 md:grid-cols-2 xl:grid-cols-[minmax(180px,1fr)_minmax(160px,220px)_auto]">
        <div className="relative">
          <Search className="absolute left-3.5 top-3 text-slate-400" size={16} />
          <Input
            className="bg-white pl-10"
            aria-label="Search resources"
            placeholder="Search resources"
            value={filters.query}
            onChange={(event) => onFilters({ query: event.target.value, page: 1 })}
          />
        </div>
        <KindSelect
          aria-label="Resource kind"
          kinds={kinds}
          emptyOption={{ label: "All kinds" }}
          className="bg-white"
          value={filters.kind}
          onChange={(event) => onFilters({ kind: event.target.value, page: 1 })}
        />
        <Toggle
          label="Deleted"
          checked={filters.includeDeleted}
          onChange={(includeDeleted) => onFilters({ includeDeleted, page: 1 })}
        />
      </div>

      <div className="min-h-0 flex-1 overflow-auto bg-white p-2">
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
            onRestore={() => onRestore(resource)}
          />
        ))}
        {!loading && listing && !listing.folders.length && !listing.resources.items.length ? (
          <div className="grid min-h-64 place-items-center rounded-2xl border border-dashed border-slate-200 bg-slate-50/50 text-sm text-slate-400">
            <span className="grid justify-items-center gap-3 font-medium">
              <span className="grid size-12 place-items-center rounded-2xl bg-white text-slate-300 shadow-sm">
                <Folder size={21} />
              </span>
              This folder is empty
            </span>
          </div>
        ) : null}
      </div>

      <footer className="flex min-h-14 items-center justify-between border-t border-slate-100 bg-slate-50/60 px-5 text-xs font-medium text-slate-500 xl:px-6">
        <span className="rounded-lg bg-white px-2.5 py-1 shadow-sm ring-1 ring-slate-200/70">
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
    ? kernel.directoryActionsAtCoreSlot(directory, coreDirectoryWorkspaceSlots.directoryContextMenu)
    : [];
  return (
    <div
      className={`group mb-1 grid min-h-[4.25rem] grid-cols-[minmax(0,1fr)_auto] items-center rounded-2xl transition-all ${selected ? "bg-indigo-50 shadow-sm ring-1 ring-inset ring-indigo-200" : "bg-amber-50/35 hover:bg-amber-50/70"}`}
    >
      <button
        className="grid min-w-0 grid-cols-[3rem_minmax(0,1fr)] items-center gap-3 px-3 py-2.5 text-left"
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
        {directory ? (
          <DirectoryThumbnail directory={directory} />
        ) : (
          <span className="grid size-12 place-items-center rounded-2xl bg-gradient-to-br from-amber-100 to-orange-100 text-amber-700 shadow-sm ring-1 ring-amber-200/70">
            <Folder size={18} />
          </span>
        )}
        <span className="min-w-0">
          <strong className="block truncate text-sm font-semibold text-slate-800">{name}</strong>
          <span className="mt-0.5 block text-[11px] font-medium text-slate-400">
            {directory?.kind ?? "Parent directory"}
          </span>
        </span>
      </button>
      {actions.length && onAction ? (
        <div className="pr-2 opacity-60 transition group-hover:opacity-100 group-focus-within:opacity-100">
          <ActionMenu>
            {actions.map((action) => (
              <ActionMenuItem
                key={action.id}
                destructive={action.ui.destructive}
                onSelect={() => onAction(action)}
              >
                {action.label}
              </ActionMenuItem>
            ))}
          </ActionMenu>
        </div>
      ) : null}
    </div>
  );
}

function DirectoryThumbnail({ directory }: { directory: Directory }) {
  const gateway = useGateway();
  const kernel = usePluginKernel();
  const action = kernel.directoryThumbnailAction(directory);
  const execute = () => {
    if (!action) throw new Error("Directory thumbnail action is unavailable");
    return gateway.executeDirectoryAction(directory, action);
  };
  const result = useQuery({
    queryKey: ["directory-thumbnail", directory.id, action?.id],
    queryFn: execute,
    enabled: Boolean(action),
    retry: false,
    staleTime: 5 * 60_000,
  });
  const image = result.data
    ? thumbnailImage(result.data.view, gateway.assetUrl.bind(gateway))
    : null;
  if (image)
    return <img className="size-12 rounded-2xl object-cover shadow-sm" src={image} alt="" />;
  return (
    <span className="grid size-12 place-items-center rounded-2xl bg-gradient-to-br from-amber-100 to-orange-100 text-amber-700 shadow-sm ring-1 ring-amber-200/70">
      <Folder className={result.isPending ? "animate-pulse" : ""} size={18} />
    </span>
  );
}

function ResourceRow({
  resource,
  selected,
  onSelect,
  onAction,
  onRestore,
}: {
  resource: Resource;
  selected: boolean;
  onSelect: () => void;
  onAction: (action: ResourceAction) => void;
  onRestore: () => void;
}) {
  const kernel = usePluginKernel();
  const actions = kernel.resourceActionsAtCoreSlot(
    resource,
    coreDirectoryWorkspaceSlots.resourceContextMenu,
  );
  return (
    <div
      className={`group mb-1 grid min-h-[4.25rem] grid-cols-[minmax(0,1fr)_auto] items-center rounded-2xl transition-all ${selected ? "bg-indigo-50 shadow-sm ring-1 ring-inset ring-indigo-200" : "hover:bg-slate-50"}`}
    >
      <button
        className="grid min-w-0 grid-cols-[3rem_minmax(0,1fr)] items-center gap-3 px-3 py-2.5 text-left"
        type="button"
        onClick={onSelect}
      >
        <ResourceThumbnail resource={resource} />
        <span className="min-w-0">
          <strong className="block truncate text-sm font-semibold text-slate-900">
            {resource.name}
          </strong>
          <span className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] font-medium text-slate-400">
            <span className="rounded-md bg-slate-100 px-1.5 py-0.5 text-slate-500">
              {resource.kind}
            </span>
            <span>{formatBytes(resource.content?.size ?? 0)}</span>
            <span>{formatDate(resource.updatedAt)}</span>
          </span>
        </span>
      </button>
      <div className="flex items-center gap-2 pr-2">
        {resource.content?.verificationStatus === "pending" ? (
          <span className="rounded-full bg-amber-50 px-2 py-1 text-[11px] font-semibold text-amber-700 ring-1 ring-amber-200/60">
            verifying
          </span>
        ) : null}
        {resource.content?.verificationStatus === "failed" ? (
          <span
            className="rounded-full bg-rose-50 px-2 py-1 text-[11px] font-semibold text-rose-700 ring-1 ring-rose-200/60"
            title={resource.content.verificationError ?? "Checksum verification failed"}
          >
            verification failed
          </span>
        ) : null}
        {resource.deletedAt ? (
          <span className="rounded-full bg-rose-50 px-2 py-1 text-[11px] font-semibold text-rose-700 ring-1 ring-rose-200/60">
            deleted
          </span>
        ) : null}
        <span className="opacity-60 transition group-hover:opacity-100 group-focus-within:opacity-100">
          <ActionMenu>
            {resource.deletedAt ? (
              <ActionMenuItem onSelect={onRestore}>Restore resource</ActionMenuItem>
            ) : null}
            {actions.map((action) => (
              <ActionMenuItem
                key={action.id}
                destructive={action.ui.destructive}
                onSelect={() => onAction(action)}
              >
                {action.label}
              </ActionMenuItem>
            ))}
          </ActionMenu>
        </span>
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
    return (
      <img className="size-12 rounded-2xl bg-slate-100 object-cover shadow-sm" src={image} alt="" />
    );
  return (
    <span className="grid size-12 place-items-center rounded-2xl border border-slate-200 bg-gradient-to-br from-white to-slate-100 text-slate-400 shadow-sm">
      <File className={result.isPending ? "animate-pulse" : ""} size={18} />
    </span>
  );
}

function thumbnailImage(
  view: import("@/domain/plugin").PluginView | null,
  resolveUrl: (url: string) => string | null,
): string | null {
  if (!view) return null;
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
    <label className="flex min-h-10 cursor-pointer items-center gap-2 rounded-xl border border-slate-200 bg-white px-3 text-sm font-medium text-slate-600 transition hover:border-slate-300">
      <input
        className="size-4 rounded accent-indigo-600"
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      {label}
    </label>
  );
}

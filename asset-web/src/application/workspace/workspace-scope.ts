import type { CurrentUser } from "@/domain/auth";
import {
  type StorageDirectory,
  storageDirectory,
  type VisibleDirectory,
  visibleDirectory,
} from "@/domain/directory-path";
import type {
  Directory,
  DirectoryListing,
  Resource,
  ResourceDraft,
  ResourceFilters,
  UploadDraft,
} from "@/domain/resource";

export type WorkspaceResource = Omit<Resource, "directory"> & { directory: VisibleDirectory };
export type WorkspaceDirectory = Omit<Directory, "path" | "parentPath"> & {
  path: VisibleDirectory;
  parentPath: VisibleDirectory;
};
export type WorkspaceDirectoryListing = Omit<DirectoryListing, "path" | "folders" | "resources"> & {
  path: VisibleDirectory;
  folders: WorkspaceDirectory[];
  resources: Omit<DirectoryListing["resources"], "items"> & { items: WorkspaceResource[] };
};
export type WorkspaceResourceFilters = Omit<ResourceFilters, "directory"> & {
  directory: VisibleDirectory;
};
export type WorkspaceResourceDraft = Omit<ResourceDraft, "directory"> & { directory: string };
export type WorkspaceUploadDraft = Omit<UploadDraft, "directory"> & { directory: string };

export interface WorkspaceScope {
  readonly root: StorageDirectory;
  readonly isGlobal: boolean;
  toStorageDirectory(path: string): StorageDirectory;
  tryToVisibleDirectory(path: string): VisibleDirectory | null;
  toVisibleDirectory(path: string): VisibleDirectory;
  toStorageFilters(filters: WorkspaceResourceFilters): ResourceFilters;
  toVisibleListing(listing: DirectoryListing): WorkspaceDirectoryListing;
  toVisibleResource(resource: Resource): WorkspaceResource;
  toStorageResource(resource: WorkspaceResource): Resource;
  toStorageResourceDraft(draft: WorkspaceResourceDraft): ResourceDraft;
  toStorageUploadDraft(draft: WorkspaceUploadDraft): UploadDraft;
}

export function createWorkspaceScope(user: CurrentUser): WorkspaceScope {
  const root = storageDirectory(user.isAdmin ? "" : user.workspaceDirectory);
  const isGlobal = !root;

  function toStorageDirectory(path: string): StorageDirectory {
    const visible = visibleDirectory(path);
    if (isGlobal) return storageDirectory(visible);
    return storageDirectory(visible ? `${root}/${visible}` : root);
  }

  function tryToVisibleDirectory(path: string): VisibleDirectory | null {
    const storage = storageDirectory(path);
    if (isGlobal) return visibleDirectory(storage);
    if (storage === root) return visibleDirectory();
    const prefix = `${root}/`;
    if (!storage.startsWith(prefix)) return null;
    return visibleDirectory(storage.slice(prefix.length));
  }

  function toVisibleDirectory(path: string): VisibleDirectory {
    const visible = tryToVisibleDirectory(path);
    if (visible !== null) return visible;
    throw new Error(`Directory is outside the current workspace: ${storageDirectory(path)}`);
  }

  function toVisibleResource(resource: Resource): WorkspaceResource {
    return { ...resource, directory: toVisibleDirectory(resource.directory) };
  }

  return {
    root,
    isGlobal,
    toStorageDirectory,
    tryToVisibleDirectory,
    toVisibleDirectory,
    toStorageFilters(filters) {
      return { ...filters, directory: toStorageDirectory(filters.directory) };
    },
    toVisibleListing(listing) {
      return {
        ...listing,
        path: toVisibleDirectory(listing.path),
        folders: listing.folders.map((folder) => ({
          ...folder,
          path: toVisibleDirectory(folder.path),
          parentPath: toVisibleDirectory(folder.parentPath),
        })),
        resources: {
          ...listing.resources,
          items: listing.resources.items.map(toVisibleResource),
        },
      };
    },
    toVisibleResource,
    toStorageResource(resource) {
      return { ...resource, directory: toStorageDirectory(resource.directory) };
    },
    toStorageResourceDraft(draft) {
      return { ...draft, directory: toStorageDirectory(draft.directory) };
    },
    toStorageUploadDraft(draft) {
      return { ...draft, directory: toStorageDirectory(draft.directory) };
    },
  };
}

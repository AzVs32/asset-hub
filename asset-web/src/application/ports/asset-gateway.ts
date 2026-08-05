import type { CurrentUser, ManagedUser, UserStatus } from "@/domain/auth";
import type { DirectoryPluginActionOutput, JsonObject, PluginActionOutput } from "@/domain/plugin";
import type {
  Directory,
  DirectoryAction,
  DirectoryKind,
  DirectoryListing,
  Resource,
  ResourceDraft,
  ResourceFilters,
  ResourceKind,
  UploadDraft,
  UploadProgress,
  UploadReceipt,
} from "@/domain/resource";

export interface AssetGateway {
  currentUser(): Promise<CurrentUser>;
  login(username: string, password: string): Promise<CurrentUser>;
  logout(): Promise<void>;

  listResourceKinds(): Promise<ResourceKind[]>;
  listDirectoryKinds(): Promise<DirectoryKind[]>;
  listDirectory(filters: ResourceFilters, signal?: AbortSignal): Promise<DirectoryListing>;
  findResource(id: string): Promise<Resource>;
  updateResource(id: string, draft: ResourceDraft): Promise<Resource>;
  restoreResource(id: string): Promise<Resource>;
  deleteResource(id: string): Promise<Resource>;
  uploadResource(
    draft: UploadDraft,
    onProgress?: (progress: UploadProgress) => void,
  ): Promise<UploadReceipt>;
  waitForUpload(id: string): Promise<Resource>;
  createDirectory(parentPath: string, name: string, kind?: string): Promise<Directory>;
  executeDirectoryAction(
    directory: Directory,
    action: DirectoryAction,
    input?: JsonObject,
  ): Promise<DirectoryPluginActionOutput>;

  executeAction(
    resource: Resource,
    actionId: string,
    input?: JsonObject,
    expectedRevision?: number,
  ): Promise<PluginActionOutput>;
  replaceResourceText(resource: Resource, text: string): Promise<Resource>;
  resourceContentUrl(resourceId: string): string;
  assetUrl(path: string): string | null;

  listUsers(): Promise<ManagedUser[]>;
  createUser(input: { username: string; password: string; isAdmin: boolean }): Promise<void>;
  updateUserStatus(id: string, status: UserStatus): Promise<ManagedUser>;
}

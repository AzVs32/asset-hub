import type { CurrentUser, ManagedUser, UserStatus } from "@/domain/auth";
import type {
  Directory,
  DirectoryKind,
  DirectoryListing,
  DirectoryPatch,
} from "@/domain/directory";
import type { DirectoryActionOutput, JsonObject, ResourceActionOutput } from "@/domain/plugin";
import type {
  Resource,
  ResourceDraft,
  ResourceFilters,
  ResourceKind,
  UploadDraft,
  UploadProgress,
  UploadReceipt,
} from "@/domain/resource";

export interface AuthGateway {
  currentUser(): Promise<CurrentUser>;
  login(username: string, password: string): Promise<CurrentUser>;
  logout(): Promise<void>;
}

export interface AssetWorkspaceGateway {
  listResourceKinds(): Promise<ResourceKind[]>;
  listDirectoryKinds(): Promise<DirectoryKind[]>;
  listDirectory(filters: ResourceFilters, signal?: AbortSignal): Promise<DirectoryListing>;
  findResource(id: string): Promise<Resource>;
  updateResource(resource: Resource, draft: ResourceDraft): Promise<Resource>;
  restoreResource(resource: Resource): Promise<Resource>;
  uploadResource(
    draft: UploadDraft,
    onProgress?: (progress: UploadProgress) => void,
  ): Promise<UploadReceipt>;
  waitForUpload(id: string): Promise<Resource>;
  createDirectory(parent: Directory, name: string, kind?: string): Promise<Directory>;
  updateDirectory(directory: Directory, patch: DirectoryPatch): Promise<Directory>;
}

export interface PluginHostGateway {
  findResource(id: string): Promise<Resource>;
  findDirectory(id: string): Promise<Directory>;
  executeDirectoryAction(
    directory: Directory,
    actionId: string,
    input?: JsonObject,
  ): Promise<DirectoryActionOutput>;
  executeResourceAction(
    resource: Resource,
    actionId: string,
    input?: JsonObject,
  ): Promise<ResourceActionOutput>;
  replaceResourceText(resource: Resource, text: string): Promise<Resource>;
  resourceContentUrl(resourceId: string): string;
  assetUrl(path: string): string | null;
}

export interface UserAdministrationGateway {
  listUsers(): Promise<ManagedUser[]>;
  createUser(input: { username: string; password: string; isAdmin: boolean }): Promise<void>;
  updateUserStatus(id: string, status: UserStatus): Promise<ManagedUser>;
}

export interface AppGateways {
  auth: AuthGateway;
  assetWorkspace: AssetWorkspaceGateway;
  pluginHost: PluginHostGateway;
  userAdministration: UserAdministrationGateway;
}

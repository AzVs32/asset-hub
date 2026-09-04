import type { CurrentUser, ManagedUser, UserStatus } from "@/domain/auth";
import type { Directory, DirectoryListing } from "@/domain/directory";
import type {
  Resource,
  ResourceDraft,
  ResourceFilters,
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
  listDirectory(filters: ResourceFilters, signal?: AbortSignal): Promise<DirectoryListing>;
  findResource(id: string): Promise<Resource>;
  updateResource(resource: Resource, draft: ResourceDraft): Promise<Resource>;
  deleteResource(resource: Resource): Promise<void>;
  resourceDownloadUrl(resource: Resource): string;
  uploadResource(
    draft: UploadDraft,
    onProgress?: (progress: UploadProgress) => void,
  ): Promise<UploadReceipt>;
  waitForUpload(id: string): Promise<Resource>;
  createDirectory(parent: Directory, name: string): Promise<Directory>;
  deleteDirectory(directory: Directory): Promise<void>;
  directoryDownloadUrl(directory: Directory): string;
}

export interface UserAdministrationGateway {
  listUsers(): Promise<ManagedUser[]>;
  createUser(input: { username: string; password: string; isAdmin: boolean }): Promise<void>;
  updateUserStatus(id: string, status: UserStatus): Promise<ManagedUser>;
}

export interface AppGateways {
  auth: AuthGateway;
  assetWorkspace: AssetWorkspaceGateway;
  userAdministration: UserAdministrationGateway;
}

import type { CurrentUser, ManagedUser, UserStatus } from "@/domain/auth";
import type { JsonObject, PluginActionOutput } from "@/domain/plugin";
import type {
  Directory,
  DirectoryListing,
  Resource,
  ResourceDraft,
  ResourceFilters,
  ResourceKind,
  UploadDraft,
} from "@/domain/resource";

export interface AssetGateway {
  currentUser(): Promise<CurrentUser>;
  login(username: string, password: string): Promise<CurrentUser>;
  logout(): Promise<void>;

  listResourceKinds(): Promise<ResourceKind[]>;
  listDirectory(filters: ResourceFilters, signal?: AbortSignal): Promise<DirectoryListing>;
  findResource(id: string): Promise<Resource>;
  createResource(draft: ResourceDraft): Promise<Resource>;
  updateResource(id: string, draft: ResourceDraft): Promise<Resource>;
  restoreResource(id: string): Promise<Resource>;
  deleteResource(id: string): Promise<Resource>;
  uploadResource(draft: UploadDraft): Promise<Resource>;
  createDirectory(parentPath: string, name: string): Promise<Directory>;

  executeAction(
    resource: Resource,
    actionId: string,
    input?: JsonObject,
  ): Promise<PluginActionOutput>;
  resourceContentUrl(resourceId: string): string;
  assetUrl(path: string): string | null;

  listUsers(): Promise<ManagedUser[]>;
  createUser(input: {
    username: string;
    password: string;
    isAdmin: boolean;
    workspaceDirectory: string;
  }): Promise<void>;
  updateUserStatus(id: string, status: UserStatus): Promise<ManagedUser>;
}

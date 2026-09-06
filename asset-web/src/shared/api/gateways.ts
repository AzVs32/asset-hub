import type { Directory, DirectoryListing } from "@/domain/directory";
import type {
  Resource,
  ResourceDraft,
  ResourceFilters,
  UploadDraft,
  UploadProgress,
  UploadReceipt,
} from "@/domain/resource";

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

export interface AppGateways {
  assetWorkspace: AssetWorkspaceGateway;
}

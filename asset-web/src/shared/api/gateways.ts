import type { Directory, DirectoryListing, DirectoryListingQuery } from "@/domain/directory";
import type { Resource, ResourceDraft } from "@/domain/resource";
import type {
  UploadDraft,
  UploadProgress,
  UploadPublication,
  UploadReceipt,
} from "@/shared/api/upload";

export interface AssetWorkspaceGateway {
  listDirectory(filters: DirectoryListingQuery, signal?: AbortSignal): Promise<DirectoryListing>;
  findResource(id: string, signal?: AbortSignal): Promise<Resource>;
  updateResource(resource: Resource, draft: ResourceDraft): Promise<Resource>;
  deleteResource(resource: Resource): Promise<void>;
  resourceDownloadUrl(resource: Resource): string;
  uploadResource(
    draft: UploadDraft,
    onProgress?: (progress: UploadProgress) => void,
  ): Promise<UploadReceipt>;
  pendingUploads(): UploadReceipt[];
  uploadStatus(id: string, signal?: AbortSignal): Promise<UploadPublication>;
  acknowledgeUpload(id: string): Promise<void>;
  createDirectory(parent: Directory, name: string): Promise<Directory>;
  deleteDirectory(directory: Directory): Promise<void>;
  directoryDownloadUrl(directory: Directory): string;
}

export interface AppGateways {
  assetWorkspace: AssetWorkspaceGateway;
}

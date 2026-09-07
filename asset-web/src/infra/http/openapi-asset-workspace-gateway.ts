import type { Directory, DirectoryListingQuery } from "@/domain/directory";
import type { Resource, ResourceDraft } from "@/domain/resource";
import type { AssetWorkspaceGateway } from "@/shared/api/gateways";
import type { UploadDraft, UploadProgress } from "@/shared/api/upload";
import type { BlobSha256, FileSha256 } from "../crypto/file-sha256";
import type { OpenApiClient } from "./openapi-client";
import { expectData, expectSuccess } from "./openapi-client";
import { mapDirectory, mapResource, resourceBody } from "./openapi-mappers";
import { ResumableUpload } from "./resumable-upload";

export class OpenApiAssetWorkspaceGateway implements AssetWorkspaceGateway {
  readonly #upload: ResumableUpload;

  constructor(
    private readonly client: OpenApiClient,
    private readonly baseUrl: string,
    hashFile: FileSha256,
    hashChunk: BlobSha256,
  ) {
    this.#upload = new ResumableUpload(
      baseUrl,
      hashFile,
      hashChunk,
      (id, signal) => this.findResource(id, signal),
      (path) => this.resolveDirectoryId(path),
    );
  }

  async listDirectory(filters: DirectoryListingQuery, signal?: AbortSignal) {
    const query = {
      path: filters.directory,
      page: filters.page,
      limit: filters.limit,
    };
    const result = await this.client.GET("/directories", {
      params: { query },
      ...(signal ? { signal } : {}),
    });
    const data = expectData(result);
    return {
      path: data.path,
      directory: mapDirectory(data.directory),
      folders: data.folders.map(mapDirectory),
      resources: {
        items: data.resources.items.map(mapResource),
        total: data.resources.total,
        page: data.resources.page,
        limit: data.resources.limit,
      },
    };
  }

  async findResource(id: string, signal?: AbortSignal) {
    const result = await this.client.GET("/resources/{id}", {
      params: { path: { id } },
      ...(signal ? { signal } : {}),
    });
    return mapResource(expectData(result));
  }

  async updateResource(resource: Resource, draft: ResourceDraft) {
    const directoryId = await this.resolveDirectoryId(draft.directory);
    const result = await this.client.PATCH("/resources/{id}", {
      params: { path: { id: resource.id } },
      body: resourceBody(draft, resource.revision, directoryId),
    });
    return mapResource(expectData(result));
  }

  async deleteResource(resource: Resource) {
    const result = await this.client.DELETE("/resources/{id}", {
      params: { path: { id: resource.id }, query: { expected_revision: resource.revision } },
    });
    expectSuccess(result);
  }

  resourceDownloadUrl(resource: Resource): string {
    return `${this.baseUrl}/resources/${encodeURIComponent(resource.id)}/download`;
  }

  uploadResource(draft: UploadDraft, onProgress?: (progress: UploadProgress) => void) {
    return this.#upload.upload(draft, onProgress);
  }

  pendingUploads() {
    return this.#upload.pendingUploads();
  }

  uploadStatus(id: string, signal?: AbortSignal) {
    return this.#upload.status(id, signal);
  }

  acknowledgeUpload(id: string) {
    return this.#upload.acknowledge(id);
  }

  private async resolveDirectoryId(path: string): Promise<string> {
    const result = await this.client.GET("/directories", {
      params: { query: { path, page: 1, limit: 1 } },
    });
    return expectData(result).directory.id;
  }

  async createDirectory(parent: Directory, name: string) {
    const result = await this.client.POST("/directories", {
      body: { parent_id: parent.id, name },
    });
    return mapDirectory(expectData(result));
  }

  async deleteDirectory(directory: Directory) {
    const result = await this.client.DELETE("/directories/{id}", {
      params: { path: { id: directory.id }, query: { expected_revision: directory.revision } },
    });
    expectSuccess(result);
  }

  directoryDownloadUrl(directory: Directory): string {
    return `${this.baseUrl}/directories/${encodeURIComponent(directory.id)}/download`;
  }
}

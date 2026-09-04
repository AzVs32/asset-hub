import type { Directory, DirectoryPatch } from "@/domain/directory";
import type {
  Resource,
  ResourceDraft,
  ResourceFilters,
  UploadDraft,
  UploadProgress,
} from "@/domain/resource";
import type { AssetWorkspaceGateway } from "@/shared/api/gateways";
import type { BlobSha256, FileSha256 } from "./file-sha256";
import type { OpenApiClient } from "./openapi-client";
import { expectData, expectSuccess } from "./openapi-client";
import {
  mapDirectory,
  mapDirectoryKind,
  mapKind,
  mapResource,
  resourceBody,
} from "./openapi-mappers";
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
      (id) => this.findResource(id),
      (path) => this.resolveDirectoryId(path),
    );
  }

  async listResourceKinds() {
    const result = await this.client.GET("/resource-kinds");
    return expectData(result).items.map(mapKind);
  }

  async listDirectoryKinds() {
    const result = await this.client.GET("/directory-kinds");
    return expectData(result).items.map(mapDirectoryKind);
  }

  async listDirectory(filters: ResourceFilters, signal?: AbortSignal) {
    const query = {
      path: filters.directory,
      page: filters.page,
      limit: filters.limit,
      ...(filters.kind ? { kind: filters.kind } : {}),
      ...(filters.query.trim() ? { q: filters.query.trim() } : {}),
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

  async findResource(id: string) {
    const result = await this.client.GET("/resources/{id}", { params: { path: { id } } });
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

  waitForUpload(id: string) {
    return this.#upload.waitForCompletion(id);
  }

  private async resolveDirectoryId(path: string): Promise<string> {
    const result = await this.client.GET("/directories", {
      params: { query: { path, page: 1, limit: 1 } },
    });
    return expectData(result).directory.id;
  }

  async createDirectory(parent: Directory, name: string, kind?: string) {
    const result = await this.client.POST("/directories", {
      body: { parent_id: parent.id, name, ...(kind ? { kind } : {}) },
    });
    return mapDirectory(expectData(result));
  }

  async updateDirectory(directory: Directory, patch: DirectoryPatch) {
    const result = await this.client.PATCH("/directories/{id}", {
      params: { path: { id: directory.id } },
      body: {
        expected_revision: directory.revision,
        ...(patch.name !== undefined ? { name: patch.name } : {}),
        ...(patch.parentId !== undefined ? { parent_id: patch.parentId } : {}),
        ...(patch.kind !== undefined ? { kind: patch.kind } : {}),
      },
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

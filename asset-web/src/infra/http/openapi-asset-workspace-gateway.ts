import type { Directory, DirectoryPatch } from "@/domain/directory";
import type { JsonObject } from "@/domain/plugin";
import type {
  Resource,
  ResourceDraft,
  ResourceFilters,
  UploadDraft,
  UploadProgress,
} from "@/domain/resource";
import type { AssetWorkspaceGateway, PluginHostGateway } from "@/shared/api/gateways";
import type { BlobSha256, FileSha256 } from "./file-sha256";
import type { components } from "./generated";
import { httpError } from "./http-error";
import { expectData, type OpenApiClient } from "./openapi-client";
import {
  mapDirectory,
  mapDirectoryActionOutput,
  mapDirectoryKind,
  mapKind,
  mapResource,
  mapResourceActionOutput,
  resourceBody,
} from "./openapi-mappers";
import { ResumableUpload } from "./resumable-upload";

type ApiResource = components["schemas"]["ResourceResponse"];

export class OpenApiAssetWorkspaceGateway implements AssetWorkspaceGateway, PluginHostGateway {
  readonly #upload: ResumableUpload;

  constructor(
    private readonly client: OpenApiClient,
    private readonly baseUrl: string,
    hashFile: FileSha256,
    private readonly hashChunk: BlobSha256,
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

  async findDirectory(id: string) {
    const result = await this.client.GET("/directories/{id}", {
      params: { path: { id } },
    });
    return mapDirectory(expectData(result));
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

  async executeDirectoryAction(directory: Directory, actionId: string, input: JsonObject = {}) {
    const action = directory.actions.find((candidate) => candidate.id === actionId);
    if (!action)
      throw new Error(`Action ${actionId} is not available for directory ${directory.id}`);
    const result = await this.client.POST("/directories/{id}/actions/{action}", {
      params: { path: { id: directory.id, action: actionId } },
      body: {
        input,
        ...(action.access === "write" ? { expected_revision: directory.revision } : {}),
      },
    });
    return mapDirectoryActionOutput(expectData(result));
  }

  async executeResourceAction(resource: Resource, actionId: string, input: JsonObject = {}) {
    const action = resource.actions.find((candidate) => candidate.id === actionId);
    if (!action) throw new Error(`Action ${actionId} is not available for resource ${resource.id}`);
    const result = await this.client.POST("/resources/{id}/actions/{action}", {
      params: { path: { id: resource.id, action: actionId } },
      body: {
        input,
        ...(action.access === "write" ? { expected_revision: resource.revision } : {}),
      },
    });
    return mapResourceActionOutput(expectData(result));
  }

  async replaceResourceText(resource: Resource, text: string) {
    const content = new Blob([text], {
      type: resource.content?.mimeType || "text/plain; charset=utf-8",
    });
    const checksum = await this.hashChunk(content);
    const response = await fetch(
      `${this.baseUrl}/resources/${encodeURIComponent(resource.id)}/content`,
      {
        method: "PUT",
        credentials: "include",
        headers: {
          "Content-Type": content.type,
          "Content-SHA256": checksum,
          "If-Match": `"${resource.revision}"`,
        },
        body: content,
      },
    );
    if (!response.ok) throw await httpError(response);
    return mapResource((await response.json()) as ApiResource);
  }

  resourceContentUrl(resourceId: string): string {
    return `${this.baseUrl}/resources/${encodeURIComponent(resourceId)}/content`;
  }

  assetUrl(path: string): string | null {
    if (!path.startsWith("/") || path.startsWith("//")) return null;
    return `${this.baseUrl}${path}`;
  }
}

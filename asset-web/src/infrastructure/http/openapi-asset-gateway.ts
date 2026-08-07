import createClient from "openapi-fetch";
import { AuthenticationRequiredError } from "@/application/errors";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { CurrentUser, ManagedUser, UserStatus } from "@/domain/auth";
import { normalizeDirectory } from "@/domain/directory-path";
import type {
  DirectoryPluginActionOutput,
  JsonObject,
  PluginActionOutput,
  PluginDiagnostic,
} from "@/domain/plugin";
import type {
  Directory,
  DirectoryAction,
  DirectoryKind,
  DirectoryListing,
  Resource,
  ResourceAction,
  ResourceDraft,
  ResourceFilters,
  ResourceKind,
  UploadDraft,
  UploadProgress,
  UploadReceipt,
} from "@/domain/resource";
import {
  type BlobSha256,
  calculateBlobSha256,
  calculateFileSha256,
  type FileSha256,
} from "./file-sha256";
import type { components, paths } from "./generated";
import { HttpError, httpError } from "./http-error";
import { isPluginViewKind, parsePluginView } from "./plugin-view-schema";

type Schemas = components["schemas"];
type ApiResource = Schemas["ResourceResponse"];
type ApiAction = Schemas["ResourceActionDefinitionResponse"];
type ApiKind = Schemas["ResourceKindResponse"];
type ApiDirectoryAction = Schemas["DirectoryActionDefinitionResponse"];

const UPLOAD_CHUNK_BYTES = 8 * 1024 * 1024;
const UPLOAD_CHUNK_CHECKSUM_ATTEMPTS = 3;
const UPLOAD_RESUME_STORAGE_KEY = "asset-hub.upload-sessions";
const UPLOAD_STATUS_POLL_MILLISECONDS = 1_000;

interface ApiUploadSession {
  id: string;
  offset: number;
  size: number;
  status: "uploading" | "finalizing" | "completed" | "failed";
  resource_id?: string;
  error?: string;
}

export class OpenApiAssetGateway implements AssetGateway {
  readonly #baseUrl: string;
  readonly #client;
  readonly #hashFile: FileSha256;
  readonly #hashChunk: BlobSha256;

  constructor(
    baseUrl = import.meta.env.VITE_API_BASE_URL || "/api",
    hashFile: FileSha256 = calculateFileSha256,
    hashChunk: BlobSha256 = calculateBlobSha256,
  ) {
    this.#baseUrl = baseUrl.replace(/\/$/, "");
    this.#client = createClient<paths>({ baseUrl: this.#baseUrl, credentials: "include" });
    this.#hashFile = hashFile;
    this.#hashChunk = hashChunk;
  }

  async currentUser(): Promise<CurrentUser> {
    try {
      const result = await this.#client.GET("/auth/me");
      return mapCurrentUser(expectData(result).user);
    } catch (error) {
      if (error instanceof HttpError && error.status === 401) {
        throw new AuthenticationRequiredError();
      }
      throw error;
    }
  }

  async login(username: string, password: string): Promise<CurrentUser> {
    const result = await this.#client.POST("/auth/login", {
      body: { username, password },
    });
    return mapCurrentUser(expectData(result).user);
  }

  async logout(): Promise<void> {
    expectSuccess(await this.#client.POST("/auth/logout"));
  }

  async listResourceKinds(): Promise<ResourceKind[]> {
    const result = await this.#client.GET("/resource-kinds");
    return expectData(result).items.map(mapKind);
  }

  async listDirectoryKinds(): Promise<DirectoryKind[]> {
    const result = await this.#client.GET("/directory-kinds");
    return expectData(result).items.map(mapDirectoryKind);
  }

  async listDirectory(filters: ResourceFilters, signal?: AbortSignal): Promise<DirectoryListing> {
    const query = {
      path: filters.directory,
      page: filters.page,
      limit: filters.limit,
      ...(filters.kind ? { kind: filters.kind } : {}),
      ...(filters.query.trim() ? { q: filters.query.trim() } : {}),
      ...(filters.includeDeleted ? { include_deleted: true } : {}),
    };
    const result = await this.#client.GET("/directories", {
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

  async findResource(id: string): Promise<Resource> {
    const result = await this.#client.GET("/resources/{id}", { params: { path: { id } } });
    return mapResource(expectData(result));
  }

  async updateResource(id: string, draft: ResourceDraft): Promise<Resource> {
    const result = await this.#client.PATCH("/resources/{id}", {
      params: { path: { id } },
      body: resourceBody(draft),
    });
    return mapResource(expectData(result));
  }

  async restoreResource(id: string): Promise<Resource> {
    const result = await this.#client.PATCH("/resources/{id}", {
      params: { path: { id } },
      body: { restore: true },
    });
    return mapResource(expectData(result));
  }

  async deleteResource(id: string): Promise<Resource> {
    const result = await this.#client.DELETE("/resources/{id}", {
      params: { path: { id } },
    });
    return mapResource(expectData(result));
  }

  async uploadResource(
    draft: UploadDraft,
    onProgress?: (progress: UploadProgress) => void,
  ): Promise<UploadReceipt> {
    const file = draft.file;
    reportUploadProgress(onProgress, "preparing", 0, file.size);
    const expectedSha256 = await this.#hashFile(file, (bytesHashed) =>
      reportUploadProgress(onProgress, "preparing", bytesHashed, file.size),
    );
    const metadata = {
      name: draft.name.length > 0 ? draft.name : file.name,
      directory: normalizeDirectory(draft.directory),
      ...(draft.kind.trim() ? { kind: draft.kind.trim() } : {}),
      mime_type: file.type || "application/octet-stream",
      size: file.size,
      expected_sha256: expectedSha256,
    };
    const fingerprint = uploadFingerprint(file, metadata, expectedSha256);
    let uploadId = loadUploadId(fingerprint);
    let offset = 0;

    if (uploadId) {
      const response = await fetch(`${this.#baseUrl}/uploads/${encodeURIComponent(uploadId)}`, {
        credentials: "include",
      });
      if (response.ok) {
        const session = parseUploadSession(await response.json());
        offset = session.offset;
        if (offset > file.size || session.size !== file.size) {
          clearUploadId(fingerprint);
          uploadId = null;
          offset = 0;
        }
      } else if (response.status === 404) {
        clearUploadId(fingerprint);
        uploadId = null;
      } else {
        throw await httpError(response);
      }
    }

    if (!uploadId) {
      const response = await fetch(`${this.#baseUrl}/uploads`, {
        method: "POST",
        credentials: "include",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(metadata),
      });
      if (!response.ok) throw await httpError(response);
      const session = parseUploadSession(await response.json());
      uploadId = session.id;
      offset = session.offset;
      saveUploadId(fingerprint, uploadId);
    }

    reportUploadProgress(onProgress, "uploading", offset, file.size);
    while (offset < file.size) {
      const chunk = file.slice(offset, Math.min(offset + UPLOAD_CHUNK_BYTES, file.size));
      const chunkChecksum = await this.#hashChunk(chunk);
      let response: Response | undefined;
      for (let attempt = 1; attempt <= UPLOAD_CHUNK_CHECKSUM_ATTEMPTS; attempt += 1) {
        response = await fetch(`${this.#baseUrl}/uploads/${encodeURIComponent(uploadId)}`, {
          method: "PATCH",
          credentials: "include",
          headers: {
            "Content-Type": "application/octet-stream",
            "Upload-Offset": String(offset),
            "Upload-Checksum": chunkChecksum,
          },
          body: chunk,
        });
        if (response.ok) break;
        const error = await httpError(response);
        const retryChecksumMismatch =
          response.status === 409 &&
          error.message.includes("upload chunk checksum mismatch") &&
          attempt < UPLOAD_CHUNK_CHECKSUM_ATTEMPTS;
        if (!retryChecksumMismatch) throw error;
      }
      if (!response?.ok) throw new Error("Upload chunk did not complete");
      const nextOffset = uploadOffset(response);
      if (nextOffset <= offset) throw new Error("Upload server did not advance the file offset");
      offset = nextOffset;
      reportUploadProgress(onProgress, "uploading", offset, file.size);
    }

    reportUploadProgress(onProgress, "finalizing", offset, file.size);
    const response = await fetch(
      `${this.#baseUrl}/uploads/${encodeURIComponent(uploadId)}/complete`,
      {
        method: "POST",
        credentials: "include",
      },
    );
    if (!response.ok) throw await httpError(response);
    parseUploadSession(await response.json());
    return { id: uploadId, name: metadata.name };
  }

  async waitForUpload(id: string): Promise<Resource> {
    for (;;) {
      const response = await fetch(`${this.#baseUrl}/uploads/${encodeURIComponent(id)}`, {
        credentials: "include",
      });
      if (!response.ok) throw await httpError(response);
      const session = parseUploadSession(await response.json());
      if (session.status === "failed") {
        throw new Error(session.error || "Resource publishing failed");
      }
      if (session.status === "completed") {
        if (!session.resource_id) {
          throw new Error("Completed upload did not include a Resource ID");
        }
        const resource = await this.findResource(session.resource_id);
        try {
          await fetch(`${this.#baseUrl}/uploads/${encodeURIComponent(id)}`, {
            method: "DELETE",
            credentials: "include",
          });
        } catch {
          // Resource 已确认创建；会话确认删除失败不影响最终结果。
        }
        clearUploadIdById(id);
        return resource;
      }
      await delay(UPLOAD_STATUS_POLL_MILLISECONDS);
    }
  }

  async createDirectory(parentPath: string, name: string, kind?: string): Promise<Directory> {
    const result = await this.#client.POST("/directories", {
      body: { parent_path: parentPath, name, ...(kind ? { kind } : {}) },
    });
    return mapDirectory(expectData(result));
  }

  async executeDirectoryAction(
    directory: Directory,
    action: DirectoryAction,
    input: JsonObject = {},
  ): Promise<DirectoryPluginActionOutput> {
    if (!directory.actions.some((candidate) => candidate.id === action.id)) {
      throw new Error(`Action ${action.id} is not available for directory ${directory.id}`);
    }
    const result = await this.#client.POST("/directories/{id}/actions/{action}", {
      params: { path: { id: directory.id, action: action.id } },
      body: { input },
    });
    const data = expectData(result);
    return {
      directoryId: data.directory_id,
      action: data.action,
      diagnostics: data.diagnostics.map(mapDiagnostic),
      view: parsePluginView(data.view),
    };
  }

  async executeAction(
    resource: Resource,
    actionId: string,
    input: JsonObject = {},
    expectedRevision?: number,
  ): Promise<PluginActionOutput> {
    const action = resource.actions.find((candidate) => candidate.id === actionId);
    if (!action) throw new Error(`Action ${actionId} is not available for resource ${resource.id}`);
    const revision =
      expectedRevision ?? (action.access === "read_write" ? resource.revision : undefined);
    const result = await this.#client.POST("/resources/{id}/actions/{action}", {
      params: { path: { id: resource.id, action: actionId } },
      body: {
        input,
        ...(revision !== undefined ? { expected_revision: revision } : {}),
      },
    });
    const data = expectData(result);
    return {
      resourceId: data.resource_id,
      action: data.action,
      diagnostics: data.diagnostics.map(mapDiagnostic),
      view: parsePluginView(data.view),
    };
  }

  async replaceResourceText(resource: Resource, text: string): Promise<Resource> {
    const content = new Blob([text], {
      type: resource.content?.mimeType || "text/plain; charset=utf-8",
    });
    const checksum = await this.#hashChunk(content);
    const response = await fetch(
      `${this.#baseUrl}/resources/${encodeURIComponent(resource.id)}/content`,
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
    return `${this.#baseUrl}/resources/${encodeURIComponent(resourceId)}/content`;
  }

  assetUrl(path: string): string | null {
    if (!path.startsWith("/") || path.startsWith("//")) return null;
    return `${this.#baseUrl}${path}`;
  }

  async listUsers(): Promise<ManagedUser[]> {
    const result = await this.#client.GET("/auth/users");
    return expectData(result).map(mapManagedUser);
  }

  async createUser(input: { username: string; password: string; isAdmin: boolean }): Promise<void> {
    expectSuccess(
      await this.#client.POST("/auth/users", {
        body: {
          username: input.username,
          password: input.password,
          is_admin: input.isAdmin,
        },
      }),
    );
  }

  async updateUserStatus(id: string, status: UserStatus): Promise<ManagedUser> {
    const result = await this.#client.PATCH("/auth/users/{id}", {
      params: { path: { id } },
      body: { status },
    });
    return mapManagedUser(expectData(result));
  }
}

type FetchResult<T> = { data?: T; error?: unknown; response: Response };

function expectData<T>(result: FetchResult<T>): T {
  if (result.data !== undefined) return result.data;
  throw apiResultError(result);
}

function expectSuccess(result: FetchResult<unknown>): void {
  if (result.response.ok) return;
  throw apiResultError(result);
}

function apiResultError(result: FetchResult<unknown>): HttpError {
  const error = result.error;
  if (error && typeof error === "object" && "error" in error) {
    const document = error as { error?: unknown; code?: unknown; details?: unknown };
    return new HttpError(
      typeof document.error === "string" ? document.error : result.response.statusText,
      result.response.status,
      typeof document.code === "string" ? document.code : null,
      document.details,
    );
  }
  return new HttpError(
    result.response.statusText || `HTTP ${result.response.status}`,
    result.response.status,
  );
}

function mapCurrentUser(value: Schemas["AuthenticatedUser"]): CurrentUser {
  return {
    id: value.id,
    username: value.username,
    role: value.is_admin ? "administrator" : "member",
    isAdmin: value.is_admin,
  };
}

function mapManagedUser(value: Schemas["ManagedUserResponse"]): ManagedUser {
  return {
    id: value.id,
    username: value.username,
    role: enumValue(value.role, ["administrator", "member"]),
    status: enumValue(value.status, ["active", "disabled"]),
    workspaceDirectory: value.workspace_directory,
  };
}

function mapDirectory(value: Schemas["DirectoryResponse"]): Directory {
  return {
    id: value.id,
    path: value.path,
    parentPath: value.parent_path,
    name: value.name,
    kind: value.kind,
    actions: value.actions.available_actions.map(mapDirectoryAction),
  };
}

function mapDirectoryKind(value: Schemas["DirectoryKindResponse"]): DirectoryKind {
  return {
    kind: value.kind,
    parent: value.parent ?? null,
    ancestors: value.ancestors,
    label: value.label,
    source: value.source,
    actions: value.actions.map(mapDirectoryAction),
  };
}

function mapDirectoryAction(value: ApiDirectoryAction): DirectoryAction {
  return {
    id: value.id,
    provides: value.provides ?? null,
    label: value.label,
    description: value.description ?? null,
    access: enumValue(value.access, ["read_only", "read_write"]),
    requires: { children: value.requires.children, resources: value.requires.resources },
    output: { views: value.output.view.filter(isPluginViewKind) },
    ui: {
      group: value.ui.group ?? null,
      order: value.ui.order ?? null,
      locations: value.ui.locations,
    },
    appliesTo: { kinds: value.applies_to.kinds },
  };
}

function mapKind(value: ApiKind): ResourceKind {
  return {
    kind: value.kind,
    parent: value.parent ?? null,
    ancestors: value.ancestors,
    label: value.label,
    supportsContent: value.supports_content,
    source: value.source,
    actions: value.actions.map(mapAction),
    detect: value.detect
      ? { mimeTypes: value.detect.mime_types, extensions: value.detect.extensions }
      : null,
  };
}

function mapResource(value: ApiResource): Resource {
  return {
    id: value.id,
    name: value.name,
    directory: value.directory,
    kind: value.kind,
    content: value.content
      ? {
          size: value.content.size,
          mimeType: value.content.mime_type ?? null,
          verificationStatus: value.content.verification_status,
          checksum: value.content.checksum ?? null,
          verificationError: value.content.verification_error ?? null,
        }
      : null,
    actions: value.actions.available_actions.map(mapAction),
    createdAt: value.created_at,
    updatedAt: value.updated_at,
    revision: value.revision,
    deletedAt: value.deleted_at ?? null,
  };
}

function mapAction(value: ApiAction): ResourceAction {
  return {
    id: value.id,
    provides: value.provides ?? null,
    label: value.label,
    description: value.description ?? null,
    access: enumValue(value.access, ["read_only", "read_write"]),
    requires: {
      content: value.requires.content,
      contentDelivery: enumValue(value.requires.content_delivery, ["auto", "inline", "reference"]),
    },
    output: { views: value.output.view.filter(isPluginViewKind) },
    ui: {
      group: value.ui.group ?? null,
      order: value.ui.order ?? null,
      locations: value.ui.locations,
    },
    appliesTo: {
      kinds: value.applies_to.kinds,
      mimeTypes: value.applies_to.mime_types,
      extensions: value.applies_to.extensions,
    },
  };
}

function mapDiagnostic(value: Schemas["PluginDiagnosticResponse"]): PluginDiagnostic {
  return {
    code: value.code,
    message: value.message,
    severity: enumValue(value.severity, ["info", "warning", "error"]),
    retryable: value.retryable,
    ...(value.details !== undefined ? { details: value.details } : {}),
  };
}

function resourceBody(draft: ResourceDraft): Schemas["UpdateResourceRequest"] {
  return {
    name: draft.name,
    directory: normalizeDirectory(draft.directory),
    kind: draft.kind,
  };
}

function uploadOffset(response: Response): number {
  const value = response.headers.get("upload-offset");
  const offset = value === null ? Number.NaN : Number(value);
  if (!Number.isSafeInteger(offset) || offset < 0) {
    throw new Error("Upload server returned an invalid Upload-Offset");
  }
  return offset;
}

function parseUploadSession(value: unknown): ApiUploadSession {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Upload server returned an invalid session");
  }
  const session = value as Record<string, unknown>;
  const statuses: ApiUploadSession["status"][] = ["uploading", "finalizing", "completed", "failed"];
  if (
    typeof session.id !== "string" ||
    !Number.isSafeInteger(session.offset) ||
    Number(session.offset) < 0 ||
    !Number.isSafeInteger(session.size) ||
    Number(session.size) < 0 ||
    !statuses.includes(session.status as ApiUploadSession["status"]) ||
    (session.resource_id !== undefined && typeof session.resource_id !== "string") ||
    (session.error !== undefined && typeof session.error !== "string")
  ) {
    throw new Error("Upload server returned an invalid session");
  }
  return session as unknown as ApiUploadSession;
}

function reportUploadProgress(
  callback: ((progress: UploadProgress) => void) | undefined,
  stage: UploadProgress["stage"],
  bytesSent: number,
  totalBytes: number,
): void {
  callback?.({ stage, bytesSent, totalBytes });
}

function uploadFingerprint(file: File, metadata: object, expectedSha256: string): string {
  return JSON.stringify({
    file: {
      name: file.name,
      size: file.size,
      lastModified: file.lastModified,
      sha256: expectedSha256,
    },
    resource: metadata,
  });
}

function loadUploadId(fingerprint: string): string | null {
  return uploadSessions()[fingerprint] ?? null;
}

function saveUploadId(fingerprint: string, id: string): void {
  const sessions = uploadSessions();
  sessions[fingerprint] = id;
  saveUploadSessions(sessions);
}

function clearUploadId(fingerprint: string): void {
  const sessions = uploadSessions();
  delete sessions[fingerprint];
  saveUploadSessions(sessions);
}

function clearUploadIdById(id: string): void {
  const sessions = Object.fromEntries(
    Object.entries(uploadSessions()).filter(([, uploadId]) => uploadId !== id),
  );
  saveUploadSessions(sessions);
}

function uploadSessions(): Record<string, string> {
  try {
    const value = globalThis.localStorage?.getItem(UPLOAD_RESUME_STORAGE_KEY);
    if (!value) return {};
    const parsed: unknown = JSON.parse(value);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(
        (entry): entry is [string, string] => typeof entry[1] === "string",
      ),
    );
  } catch {
    return {};
  }
}

function saveUploadSessions(sessions: Record<string, string>): void {
  try {
    globalThis.localStorage?.setItem(UPLOAD_RESUME_STORAGE_KEY, JSON.stringify(sessions));
  } catch {
    // 浏览器禁用持久化时仍允许当前上传继续。
  }
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => globalThis.setTimeout(resolve, milliseconds));
}

function enumValue<const T extends string>(value: string, values: readonly T[]): T {
  const match = values.find((candidate) => candidate === value);
  if (!match) throw new Error(`Unexpected API enum value: ${value}`);
  return match;
}

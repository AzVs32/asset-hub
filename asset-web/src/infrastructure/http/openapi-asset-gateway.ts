import createClient from "openapi-fetch";
import { AuthenticationRequiredError } from "@/application/errors";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { CurrentUser, ManagedUser, UserStatus } from "@/domain/auth";
import { normalizeDirectory } from "@/domain/directory-path";
import type { JsonObject, PluginActionOutput, PluginDiagnostic } from "@/domain/plugin";
import type {
  Directory,
  DirectoryListing,
  Resource,
  ResourceAction,
  ResourceDraft,
  ResourceFilters,
  ResourceKind,
  UploadDraft,
} from "@/domain/resource";
import type { components, paths } from "./generated";
import { HttpError, httpError } from "./http-error";
import { isPluginViewKind, parsePluginView } from "./plugin-view-schema";

type Schemas = components["schemas"];
type ApiResource = Schemas["ResourceResponse"];
type ApiAction = Schemas["ResourceActionDefinitionResponse"];
type ApiKind = Schemas["ResourceKindResponse"];

export class OpenApiAssetGateway implements AssetGateway {
  readonly #baseUrl: string;
  readonly #client;

  constructor(baseUrl = import.meta.env.VITE_API_BASE_URL || "/api") {
    this.#baseUrl = baseUrl.replace(/\/$/, "");
    this.#client = createClient<paths>({ baseUrl: this.#baseUrl, credentials: "include" });
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

  async listDirectory(filters: ResourceFilters, signal?: AbortSignal): Promise<DirectoryListing> {
    const query = {
      path: filters.directory,
      page: filters.page,
      limit: filters.limit,
      ...(filters.kind ? { kind: filters.kind } : {}),
      ...(filters.kind && filters.includeDescendants ? { include_descendants: true } : {}),
      ...(filters.tag.trim() ? { tag: filters.tag.trim() } : {}),
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

  async createResource(draft: ResourceDraft): Promise<Resource> {
    const result = await this.#client.POST("/resources", { body: resourceBody(draft) });
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
      body: { restore: true, status: "active" },
    });
    return mapResource(expectData(result));
  }

  async deleteResource(id: string): Promise<Resource> {
    const result = await this.#client.DELETE("/resources/{id}", {
      params: { path: { id } },
    });
    return mapResource(expectData(result));
  }

  async uploadResource(draft: UploadDraft): Promise<Resource> {
    const params = new URLSearchParams({
      name: draft.name.length > 0 ? draft.name : draft.file.name,
      directory: normalizeDirectory(draft.directory),
      tags_json: JSON.stringify(splitTags(draft.tags)),
    });
    if (draft.description.trim()) params.set("description", draft.description.trim());
    if (draft.kind.trim()) params.set("kind", draft.kind.trim());
    const response = await fetch(`${this.#baseUrl}/resources/content/stream?${params}`, {
      method: "PUT",
      credentials: "include",
      headers: { "Content-Type": draft.file.type || "application/octet-stream" },
      body: draft.file,
    });
    if (!response.ok) throw await httpError(response);
    return mapResource((await response.json()) as ApiResource);
  }

  async createDirectory(parentPath: string, name: string): Promise<Directory> {
    const result = await this.#client.POST("/directories", {
      body: { parent_path: parentPath, name },
    });
    return mapDirectory(expectData(result));
  }

  async executeAction(
    resource: Resource,
    actionId: string,
    input: JsonObject = {},
  ): Promise<PluginActionOutput> {
    const action = resource.actions.find((candidate) => candidate.id === actionId);
    if (!action) throw new Error(`Action ${actionId} is not available for resource ${resource.id}`);
    if (isContentFallbackAction(resource, action)) {
      return {
        resourceId: resource.id,
        action: action.id,
        diagnostics: [],
        view: {
          view: "binary_url",
          url: `/resources/${encodeURIComponent(resource.id)}/content`,
          ...(resource.content?.mimeType ? { mime_type: resource.content.mimeType } : {}),
          filename: resource.name,
        },
      };
    }
    const result = await this.#client.POST("/resources/{id}/actions/{action}", {
      params: { path: { id: resource.id, action: actionId } },
      body: { input },
    });
    const data = expectData(result);
    return {
      resourceId: data.resource_id,
      action: data.action,
      diagnostics: data.diagnostics.map(mapDiagnostic),
      view: parsePluginView(data.view),
    };
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
  return { id: value.id, path: value.path, parentPath: value.parent_path, name: value.name };
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
    status: enumValue(value.status, ["active", "archived"]),
    description: value.description ?? null,
    tags: value.tags,
    content: value.content
      ? {
          size: value.content.size,
          mimeType: value.content.mime_type ?? null,
          checksum: value.content.checksum,
        }
      : null,
    actions: value.actions.available_actions.map(mapAction),
    createdAt: value.created_at,
    updatedAt: value.updated_at,
    deletedAt: value.deleted_at ?? null,
  };
}

function mapAction(value: ApiAction): ResourceAction {
  return {
    id: value.id,
    label: value.label,
    description: value.description ?? null,
    access: enumValue(value.access, ["read_only", "read_write"]),
    executor: {
      type: enumValue(value.executor.type, ["builtin", "plugin"]),
      handler: value.executor.handler ?? null,
    },
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

function resourceBody(draft: ResourceDraft): Schemas["CreateResourceRequest"] {
  return {
    name: draft.name,
    directory: normalizeDirectory(draft.directory),
    kind: draft.kind,
    status: draft.status,
    description: draft.description.trim() || null,
    tags: splitTags(draft.tags),
  };
}

function splitTags(value: string): string[] {
  return [
    ...new Set(
      value
        .split(",")
        .map((tag) => tag.trim())
        .filter(Boolean),
    ),
  ];
}

function enumValue<const T extends string>(value: string, values: readonly T[]): T {
  const match = values.find((candidate) => candidate === value);
  if (!match) throw new Error(`Unexpected API enum value: ${value}`);
  return match;
}

function isContentFallbackAction(resource: Resource, action: ResourceAction): boolean {
  return Boolean(
    resource.content &&
      action.executor.type === "builtin" &&
      !action.executor.handler &&
      action.access === "read_only" &&
      action.output.views.length === 0,
  );
}

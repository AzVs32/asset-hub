import type { CurrentUser, ManagedUser } from "@/domain/auth";
import type { Directory } from "@/domain/directory";
import type { Resource, ResourceDraft } from "@/domain/resource";
import type { components } from "./generated";

type Schemas = components["schemas"];
type ApiResource = Schemas["ResourceResponse"];

export function mapCurrentUser(value: Schemas["AuthenticatedUser"]): CurrentUser {
  return {
    id: value.id,
    username: value.username,
    role: value.is_admin ? "administrator" : "member",
    isAdmin: value.is_admin,
  };
}

export function mapManagedUser(value: Schemas["ManagedUserResponse"]): ManagedUser {
  return {
    id: value.id,
    username: value.username,
    role: enumValue(value.role, ["administrator", "member"]),
    status: enumValue(value.status, ["active", "disabled"]),
    workspaceDirectory: value.workspace_directory,
  };
}

export function mapDirectory(value: Schemas["DirectoryResponse"]): Directory {
  return {
    id: value.id,
    parentId: value.parent_id ?? null,
    path: value.path,
    parentPath: value.parent_path,
    name: value.name,
    createdAt: value.created_at,
    updatedAt: value.updated_at,
    revision: value.revision,
  };
}

export function mapResource(value: ApiResource): Resource {
  return {
    id: value.id,
    name: value.name,
    directoryId: value.directory_id,
    directory: value.directory,
    state: {
      lifecycle: value.state.lifecycle,
      content: value.state.content,
      effective: value.state.effective,
    },
    content: value.content
      ? {
          size: value.content.size,
          mimeType: value.content.mime_type ?? null,
          checksum: value.content.checksum ?? null,
          verificationError: value.content.verification_error ?? null,
        }
      : null,
    createdAt: value.created_at,
    updatedAt: value.updated_at,
    revision: value.revision,
  };
}

export function resourceBody(
  draft: ResourceDraft,
  expectedRevision: number,
  directoryId: string,
): Schemas["UpdateResourceRequest"] {
  return {
    expected_revision: expectedRevision,
    name: draft.name,
    directory_id: directoryId,
  };
}

function enumValue<const T extends string>(value: string, values: readonly T[]): T {
  const match = values.find((candidate) => candidate === value);
  if (!match) throw new Error(`Unexpected API enum value: ${value}`);
  return match;
}

import type { CurrentUser, ManagedUser } from "@/domain/auth";
import type { DefinitionOrigin } from "@/domain/definition";
import type { Directory, DirectoryKind } from "@/domain/directory";
import type { Resource, ResourceDraft, ResourceKind } from "@/domain/resource";
import type { components } from "./generated";

type Schemas = components["schemas"];
type ApiResource = Schemas["ResourceResponse"];
type ApiKind = Schemas["ResourceKindResponse"];

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
    kind: value.kind,
    createdAt: value.created_at,
    updatedAt: value.updated_at,
    revision: value.revision,
  };
}

export function mapDirectoryKind(value: Schemas["DirectoryKindResponse"]): DirectoryKind {
  return {
    kind: value.kind,
    parent: value.parent ?? null,
    ancestors: value.ancestors,
    allowedParentKinds: value.allowed_parent_kinds,
    label: value.label,
    origin: mapOrigin(value.origin),
  };
}

export function mapKind(value: ApiKind): ResourceKind {
  return {
    kind: value.kind,
    parent: value.parent ?? null,
    ancestors: value.ancestors,
    label: value.label,
    supportsContent: value.supports_content,
    origin: mapOrigin(value.origin),
    detect: value.detect
      ? { mimeTypes: value.detect.mime_types, extensions: value.detect.extensions }
      : null,
  };
}

export function mapResource(value: ApiResource): Resource {
  return {
    id: value.id,
    name: value.name,
    directoryId: value.directory_id,
    directory: value.directory,
    kind: value.kind,
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
    kind: draft.kind,
  };
}

function mapOrigin(value: Schemas["DefinitionOriginResponse"]): DefinitionOrigin {
  return {
    kind: enumValue(value.kind, ["builtin", "plugin"]),
    id: value.id,
  };
}

function enumValue<const T extends string>(value: string, values: readonly T[]): T {
  const match = values.find((candidate) => candidate === value);
  if (!match) throw new Error(`Unexpected API enum value: ${value}`);
  return match;
}

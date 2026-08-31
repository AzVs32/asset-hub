import type { DefinitionOrigin } from "@/domain/action";
import type { CurrentUser, ManagedUser } from "@/domain/auth";
import type { Directory, DirectoryAction, DirectoryKind } from "@/domain/directory";
import { normalizeDirectory } from "@/domain/directory-path";
import {
  type DirectoryActionEffectKind,
  type DirectoryActionOutput,
  directoryActionCapabilityIds,
  directoryActionEffectKinds,
  type JsonValue,
  type PluginDiagnostic,
  type ResourceActionEffectKind,
  type ResourceActionOutput,
  resourceActionCapabilityIds,
  resourceActionEffectKinds,
} from "@/domain/plugin";
import type { Resource, ResourceAction, ResourceDraft, ResourceKind } from "@/domain/resource";
import type { components } from "./generated";
import { isPluginViewKind, parseOptionalPluginView } from "./plugin-view-schema";

type Schemas = components["schemas"];
type ApiResource = Schemas["ResourceResponse"];
type ApiAction = Schemas["ResourceActionDefinitionResponse"];
type ApiKind = Schemas["ResourceKindResponse"];
type ApiDirectoryAction = Schemas["DirectoryActionDefinitionResponse"];

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
    actions: value.actions.map(mapDirectoryAction),
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
    actions: value.actions.map(mapDirectoryAction),
  };
}

function mapDirectoryAction(value: ApiDirectoryAction): DirectoryAction {
  return {
    id: value.id,
    origin: mapOrigin(value.origin),
    provides:
      value.provides === undefined || value.provides === null
        ? null
        : enumValue(value.provides, directoryActionCapabilityIds),
    label: value.label,
    description: value.description ?? null,
    access: enumValue(value.access, ["read", "write"]),
    requires: {
      children: value.requires.children,
      resources: enumValue(value.requires.resources, ["none", "metadata", "content"]),
    },
    output: {
      views: value.output.views.filter(isPluginViewKind),
      effects: value.output.effects.map(directoryEffectKind),
    },
    ui: {
      group: value.ui.group ?? null,
      order: value.ui.order ?? null,
      locations: value.ui.locations,
      destructive: value.ui.destructive,
      confirmation: value.ui.confirmation ?? null,
    },
    appliesTo: { kinds: value.applies_to.kinds },
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
    actions: value.actions.map(mapAction),
    detect: value.detect
      ? { mimeTypes: value.detect.mime_types, extensions: value.detect.extensions }
      : null,
  };
}

export function mapResource(value: ApiResource): Resource {
  return {
    id: value.id,
    name: value.name,
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
    actions: value.actions.map(mapAction),
    createdAt: value.created_at,
    updatedAt: value.updated_at,
    revision: value.revision,
  };
}

function mapAction(value: ApiAction): ResourceAction {
  return {
    id: value.id,
    origin: mapOrigin(value.origin),
    provides:
      value.provides === undefined || value.provides === null
        ? null
        : enumValue(value.provides, resourceActionCapabilityIds),
    label: value.label,
    description: value.description ?? null,
    access: enumValue(value.access, ["read", "write"]),
    requires: {
      content: value.requires.content,
      contentDelivery: enumValue(value.requires.content_delivery, ["auto", "inline", "reference"]),
    },
    output: {
      views: value.output.views.filter(isPluginViewKind),
      effects: value.output.effects.map(resourceEffectKind),
    },
    ui: {
      group: value.ui.group ?? null,
      order: value.ui.order ?? null,
      locations: value.ui.locations,
      destructive: value.ui.destructive,
      confirmation: value.ui.confirmation ?? null,
    },
    appliesTo: {
      kinds: value.applies_to.kinds,
      mimeTypes: value.applies_to.mime_types,
      extensions: value.applies_to.extensions,
    },
  };
}

export function mapResourceActionOutput(
  data: Schemas["ResourceActionOutputResponse"],
): ResourceActionOutput {
  return {
    resourceId: data.resource_id,
    action: data.action,
    diagnostics: data.diagnostics.map(mapDiagnostic),
    view: parseOptionalPluginView(data.view),
    effects: data.effects.map(resourceEffectKind),
  };
}

export function mapDirectoryActionOutput(
  data: Schemas["DirectoryActionOutputResponse"],
): DirectoryActionOutput {
  return {
    directoryId: data.directory_id,
    action: data.action,
    diagnostics: data.diagnostics.map(mapDiagnostic),
    view: parseOptionalPluginView(data.view),
    effects: data.effects.map(directoryEffectKind),
  };
}

export function resourceBody(
  draft: ResourceDraft,
  expectedRevision: number,
): Schemas["UpdateResourceRequest"] {
  return {
    expected_revision: expectedRevision,
    name: draft.name,
    directory: normalizeDirectory(draft.directory),
    kind: draft.kind,
  };
}

function resourceEffectKind(value: string): ResourceActionEffectKind {
  return enumValue(value, resourceActionEffectKinds);
}

function directoryEffectKind(value: string): DirectoryActionEffectKind {
  return enumValue(value, directoryActionEffectKinds);
}

function mapDiagnostic(value: Schemas["PluginDiagnosticResponse"]): PluginDiagnostic {
  return {
    code: value.code,
    message: value.message,
    severity: enumValue(value.severity, ["info", "warning", "error"]),
    retryable: value.retryable,
    ...(value.details !== undefined ? { details: value.details as JsonValue } : {}),
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

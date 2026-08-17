/** Current Host/plugin wire and Browser Frame API version. */
export const PLUGIN_API_VERSION = "asset-hub.plugin-api@1";

/** Penpal channel used by Resource-bound plugin frames. */
export const RESOURCE_FRAME_CHANNEL = "asset-hub.plugin-frame@1";

/** Penpal channel used by Directory-bound plugin frames. */
export const DIRECTORY_FRAME_CHANNEL = "asset-hub.plugin-directory-frame@1";

/** Resource Action singleton capability IDs supported by Manifest version 3. */
export const resourceActionCapabilityIds = ["thumbnail", "view", "edit"] as const;
export type ResourceActionCapabilityId = (typeof resourceActionCapabilityIds)[number];
export const RESOURCE_THUMBNAIL_CAPABILITY = resourceActionCapabilityIds[0];
export const RESOURCE_VIEW_CAPABILITY = resourceActionCapabilityIds[1];
export const RESOURCE_EDIT_CAPABILITY = resourceActionCapabilityIds[2];

/** Directory Action singleton capability IDs supported by Manifest version 3. */
export const directoryActionCapabilityIds = ["thumbnail", "workspace"] as const;
export type DirectoryActionCapabilityId = (typeof directoryActionCapabilityIds)[number];
export const DIRECTORY_THUMBNAIL_CAPABILITY = directoryActionCapabilityIds[0];
export const DIRECTORY_WORKSPACE_CAPABILITY = directoryActionCapabilityIds[1];

/** Host methods exposed to Resource-bound plugin frames. */
export const resourceFrameMethods = ["executeResourceAction", "replaceResourceText"] as const;

/** Host methods exposed to Directory-bound plugin frames. */
export const directoryFrameMethods = [
  "executeDirectoryAction",
  "viewResource",
  "refreshDirectory",
  "navigateToDirectory",
  "editResource",
] as const;

export type JsonPrimitive = boolean | number | string | null;
export type JsonValue = JsonPrimitive | JsonValue[] | JsonObject;
export type JsonObject = { [key: string]: JsonValue };

export const pluginViewKinds = [
  "text",
  "markdown",
  "html",
  "plugin_frame",
  "json",
  "media",
  "download",
] as const;

export type PluginViewKind = (typeof pluginViewKinds)[number];

export type PluginView =
  | { view: "text"; text: string }
  | { view: "markdown"; markdown: string }
  | { view: "html"; title?: string; html: string }
  | { view: "plugin_frame"; plugin_api: string; title?: string; url: string }
  | { view: "json"; data: JsonValue }
  | {
      view: "media";
      mime_type: string;
      title?: string;
      encoding: "base64" | "url";
      data: string;
    }
  | { view: "download"; url: string; mime_type?: string; filename?: string };

export interface PluginDiagnostic {
  code: string;
  message: string;
  severity: "info" | "warning" | "error";
  retryable: boolean;
  details?: JsonValue;
}

export const resourceActionEffectKinds = ["replace_content", "delete"] as const;
export type ResourceActionEffectKind = (typeof resourceActionEffectKinds)[number];

export const directoryActionEffectKinds = [
  "update",
  "create_child",
  "create_tree",
  "delete",
] as const;
export type DirectoryActionEffectKind = (typeof directoryActionEffectKinds)[number];

/** Host-normalized result returned to a Resource-bound browser frame. */
export interface ResourceActionOutput {
  resourceId: string;
  action: string;
  view: PluginView | null;
  effects: ResourceActionEffectKind[];
  diagnostics: PluginDiagnostic[];
}

/** Host-normalized result returned to a Directory-bound browser frame. */
export interface DirectoryActionOutput {
  directoryId: string;
  action: string;
  view: PluginView | null;
  effects: DirectoryActionEffectKind[];
  diagnostics: PluginDiagnostic[];
}

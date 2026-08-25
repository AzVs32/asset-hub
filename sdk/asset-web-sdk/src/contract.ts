import {
  DIRECTORY_FRAME_CHANNEL,
  directoryActionCapabilityIds,
  directoryActionEffectKinds,
  directoryFrameMethods,
  PLUGIN_API_VERSION,
  pluginViewKinds,
  RESOURCE_FRAME_CHANNEL,
  resourceActionCapabilityIds,
  resourceActionEffectKinds,
  resourceFrameMethods,
} from "./contract.generated";

export {
  DIRECTORY_FRAME_CHANNEL,
  directoryActionCapabilityIds,
  directoryActionEffectKinds,
  directoryFrameMethods,
  PLUGIN_API_VERSION,
  pluginViewKinds,
  RESOURCE_FRAME_CHANNEL,
  resourceActionCapabilityIds,
  resourceActionEffectKinds,
  resourceFrameMethods,
};

export type ResourceActionCapabilityId = (typeof resourceActionCapabilityIds)[number];
export const RESOURCE_THUMBNAIL_CAPABILITY = resourceActionCapabilityIds[0];
export const RESOURCE_VIEW_CAPABILITY = resourceActionCapabilityIds[1];
export const RESOURCE_EDIT_CAPABILITY = resourceActionCapabilityIds[2];

export type DirectoryActionCapabilityId = (typeof directoryActionCapabilityIds)[number];
export const DIRECTORY_THUMBNAIL_CAPABILITY = directoryActionCapabilityIds[0];
export const DIRECTORY_WORKSPACE_CAPABILITY = directoryActionCapabilityIds[1];

export type JsonPrimitive = boolean | number | string | null;
export type JsonValue = JsonPrimitive | JsonValue[] | JsonObject;
export type JsonObject = { [key: string]: JsonValue };

export type PluginViewKind = (typeof pluginViewKinds)[number];

export type PluginView =
  | { type: "text"; text: string }
  | { type: "markdown"; markdown: string }
  | { type: "html"; title?: string; html: string }
  | { type: "plugin_frame"; plugin_api: string; title?: string; url: string }
  | { type: "json"; data: JsonValue }
  | {
      type: "media";
      mime_type: string;
      title?: string;
      encoding: "base64" | "url";
      data: string;
    }
  | { type: "download"; url: string; mime_type?: string; filename?: string };

export interface PluginDiagnostic {
  code: string;
  message: string;
  severity: "info" | "warning" | "error";
  retryable: boolean;
  details?: JsonValue;
}

export type ResourceActionEffectKind = (typeof resourceActionEffectKinds)[number];
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

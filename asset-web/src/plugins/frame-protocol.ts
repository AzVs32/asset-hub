import type { JsonObject } from "@/domain/plugin";

export interface ExecuteActionMessage {
  type: "asset-hub:execute-resource-action";
  pluginApi: string;
  requestId: string;
  resourceId: string;
  action: string;
  input?: JsonObject;
}

export interface ReplaceResourceTextMessage {
  type: "asset-hub:replace-resource-text";
  pluginApi: string;
  requestId: string;
  resourceId: string;
  text: string;
}

export type PluginFrameRequest = ExecuteActionMessage | ReplaceResourceTextMessage;

export function parsePluginFrameRequest(
  value: unknown,
  expectedPluginApi: string,
): PluginFrameRequest | null {
  const fields = parseCommonFields(value, expectedPluginApi);
  if (!fields) return null;
  const message = value as Record<string, unknown>;
  if (message.type === "asset-hub:execute-resource-action") {
    const action = message.action;
    if (typeof action !== "string" || !action || action.length > 128) return null;
    const input = asJsonObject(message.input);
    if (message.input !== undefined && !input) return null;
    return {
      type: message.type,
      ...fields,
      action,
      ...(input ? { input } : {}),
    };
  }
  if (message.type === "asset-hub:replace-resource-text") {
    if (typeof message.text !== "string") return null;
    return { type: message.type, ...fields, text: message.text };
  }
  return null;
}

export function parseExecuteActionMessage(
  value: unknown,
  expectedPluginApi: string,
): ExecuteActionMessage | null {
  const message = parsePluginFrameRequest(value, expectedPluginApi);
  return message?.type === "asset-hub:execute-resource-action" ? message : null;
}

function parseCommonFields(
  value: unknown,
  expectedPluginApi: string,
): Pick<PluginFrameRequest, "pluginApi" | "requestId" | "resourceId"> | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const message = value as Record<string, unknown>;
  if (message.plugin_api !== expectedPluginApi) return null;
  const requestId = message.request_id;
  const resourceId = message.resource_id;
  if (typeof requestId !== "string" || !requestId || requestId.length > 128) return null;
  if (typeof resourceId !== "string" || !resourceId || resourceId.length > 128) return null;
  return { pluginApi: expectedPluginApi, requestId, resourceId };
}

function asJsonObject(value: unknown): JsonObject | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonObject) : null;
}

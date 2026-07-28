import type { JsonObject } from "@/domain/plugin";

export interface ExecuteActionMessage {
  type: "asset-hub:execute-resource-action";
  pluginApi: string;
  requestId: string;
  resourceId: string;
  action: string;
  input?: JsonObject;
}

export function parseExecuteActionMessage(
  value: unknown,
  expectedPluginApi: string,
): ExecuteActionMessage | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const message = value as Record<string, unknown>;
  if (message.type !== "asset-hub:execute-resource-action") return null;
  if (message.plugin_api !== expectedPluginApi) return null;
  const requestId = message.request_id;
  const resourceId = message.resource_id;
  const action = message.action;
  if (typeof requestId !== "string" || !requestId || requestId.length > 128) return null;
  if (typeof resourceId !== "string" || !resourceId || resourceId.length > 128) return null;
  if (typeof action !== "string" || !action || action.length > 128) return null;
  const input = asJsonObject(message.input);
  if (message.input !== undefined && !input) return null;
  return {
    type: message.type,
    pluginApi: expectedPluginApi,
    requestId,
    resourceId,
    action,
    ...(input ? { input } : {}),
  };
}

function asJsonObject(value: unknown): JsonObject | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonObject) : null;
}

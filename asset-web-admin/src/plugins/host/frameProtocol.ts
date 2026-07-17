import type { JsonObject } from "./contracts";

export const pluginFrameProtocolVersion = 1 as const;

export type ExecuteActionMessage = {
  type: "asset-hub:execute-resource-action";
  version?: 1;
  request_id: string;
  resource_id: string;
  action: string;
  input?: JsonObject;
};

export function parseExecuteActionMessage(value: unknown): ExecuteActionMessage | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const message = value as Partial<ExecuteActionMessage>;
  if (message.type !== "asset-hub:execute-resource-action") return null;
  if (message.version !== undefined && message.version !== pluginFrameProtocolVersion) return null;
  if (typeof message.request_id !== "string" || message.request_id.length === 0 || message.request_id.length > 128) return null;
  if (typeof message.resource_id !== "string" || message.resource_id.length > 128) return null;
  if (typeof message.action !== "string" || message.action.length > 128) return null;
  if (message.input !== undefined && !isJsonObject(message.input)) return null;
  return message as ExecuteActionMessage;
}

function isJsonObject(value: unknown): value is JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}


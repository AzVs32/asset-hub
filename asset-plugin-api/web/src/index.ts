import { CallOptions, connect, WindowMessenger } from "penpal";

export const PLUGIN_API_VERSION = "asset-hub.plugin-api@5";

const FRAME_CHANNEL = "asset-hub.plugin-frame@5";
const DIRECTORY_FRAME_CHANNEL = "asset-hub.plugin-directory-frame@5";
const defaultConnectionTimeoutMs = 10_000;
const defaultCallTimeoutMs = 30_000;

export type JsonPrimitive = boolean | number | string | null;
export type JsonValue = JsonPrimitive | JsonValue[] | JsonObject;
export type JsonObject = { [key: string]: JsonValue };

export type PluginView =
  | { view: "text"; text: string }
  | { view: "markdown"; markdown: string }
  | { view: "html"; title?: string; html: string }
  | { view: "plugin_frame"; plugin_api: string; title?: string; url: string }
  | { view: "json"; data: unknown }
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
  details?: unknown;
}

export interface ResourceActionOutput {
  resourceId: string;
  action: string;
  view: PluginView;
  diagnostics: PluginDiagnostic[];
}

export interface DirectoryActionOutput {
  directoryId: string;
  action: string;
  view: PluginView | null;
  effects: Array<"update" | "create_child" | "create_tree" | "delete">;
  diagnostics: PluginDiagnostic[];
}

interface AssetHubFrameHost extends Record<string, (...args: never[]) => unknown> {
  executeResourceAction(action: string, input?: JsonObject): Promise<ResourceActionOutput>;
  replaceResourceText(text: string): Promise<void>;
}

export interface AssetHubFrameClient {
  executeResourceAction(action: string, input?: JsonObject): Promise<ResourceActionOutput>;
  replaceResourceText(text: string): Promise<void>;
  disconnect(): void;
}

interface AssetHubDirectoryFrameHost extends Record<string, (...args: never[]) => unknown> {
  executeDirectoryAction(action: string, input?: JsonObject): Promise<DirectoryActionOutput>;
  refreshDirectory(): Promise<void>;
  navigateToDirectory(path: string): Promise<void>;
}

export interface AssetHubDirectoryFrameClient {
  executeDirectoryAction(action: string, input?: JsonObject): Promise<DirectoryActionOutput>;
  refreshDirectory(): Promise<void>;
  navigateToDirectory(path: string): Promise<void>;
  disconnect(): void;
}

export interface AssetHubFrameConnectionOptions {
  connectionTimeoutMs?: number;
  callTimeoutMs?: number;
}

/** Connects the current plugin iframe to the narrow capability API exposed by its Asset Hub host. */
export async function connectAssetHubFrame(
  options: AssetHubFrameConnectionOptions = {},
): Promise<AssetHubFrameClient> {
  if (window.parent === window) {
    throw new Error("Asset Hub Plugin Web SDK must run inside a plugin frame.");
  }
  const connectionTimeoutMs = positiveTimeout(
    options.connectionTimeoutMs,
    defaultConnectionTimeoutMs,
    "connectionTimeoutMs",
  );
  const callTimeoutMs = positiveTimeout(
    options.callTimeoutMs,
    defaultCallTimeoutMs,
    "callTimeoutMs",
  );
  const messenger = new WindowMessenger({
    remoteWindow: window.parent,
    // Plugin frames intentionally omit allow-same-origin, so their origin is opaque.
    // Penpal still restricts messages to this exact parent Window reference.
    allowedOrigins: ["*"],
  });
  const connection = connect<AssetHubFrameHost>({
    messenger,
    channel: FRAME_CHANNEL,
    timeout: connectionTimeoutMs,
  });
  const host = await connection.promise;
  return {
    executeResourceAction(action, input) {
      const callOptions = new CallOptions({ timeout: callTimeoutMs });
      return host.executeResourceAction(action, input ?? {}, callOptions);
    },
    replaceResourceText(text) {
      return host.replaceResourceText(text, new CallOptions({ timeout: callTimeoutMs }));
    },
    disconnect() {
      connection.destroy();
    },
  };
}

/** Connects a Directory workspace iframe to capabilities bound to its current Directory. */
export async function connectAssetHubDirectoryFrame(
  options: AssetHubFrameConnectionOptions = {},
): Promise<AssetHubDirectoryFrameClient> {
  if (window.parent === window) {
    throw new Error("Asset Hub Directory Plugin Web SDK must run inside a plugin frame.");
  }
  const connectionTimeoutMs = positiveTimeout(
    options.connectionTimeoutMs,
    defaultConnectionTimeoutMs,
    "connectionTimeoutMs",
  );
  const callTimeoutMs = positiveTimeout(
    options.callTimeoutMs,
    defaultCallTimeoutMs,
    "callTimeoutMs",
  );
  const messenger = new WindowMessenger({
    remoteWindow: window.parent,
    allowedOrigins: ["*"],
  });
  const connection = connect<AssetHubDirectoryFrameHost>({
    messenger,
    channel: DIRECTORY_FRAME_CHANNEL,
    timeout: connectionTimeoutMs,
  });
  const host = await connection.promise;
  return {
    executeDirectoryAction(action, input) {
      return host.executeDirectoryAction(
        action,
        input ?? {},
        new CallOptions({ timeout: callTimeoutMs }),
      );
    },
    refreshDirectory() {
      return host.refreshDirectory(new CallOptions({ timeout: callTimeoutMs }));
    },
    navigateToDirectory(path) {
      return host.navigateToDirectory(path, new CallOptions({ timeout: callTimeoutMs }));
    },
    disconnect() {
      connection.destroy();
    },
  };
}

function positiveTimeout(value: number | undefined, fallback: number, name: string): number {
  if (value === undefined) return fallback;
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new TypeError(`${name} must be a positive safe integer.`);
  }
  return value;
}

import { CallOptions, connect, WindowMessenger } from "penpal";
import {
  DIRECTORY_FRAME_CHANNEL,
  type DirectoryActionOutput,
  type JsonObject,
  PLUGIN_API_VERSION,
  RESOURCE_FRAME_CHANNEL,
  type ResourceActionOutput,
} from "./contract";

export * from "./contract";

const defaultConnectionTimeoutMs = 10_000;
const defaultCallTimeoutMs = 30_000;

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
  viewResource(resourceId: string, input?: JsonObject): Promise<ResourceActionOutput>;
  refreshDirectory(): Promise<void>;
  navigateToDirectory(path: string): Promise<void>;
  editResource(resourceId: string): Promise<void>;
}

export interface AssetHubDirectoryFrameClient {
  executeDirectoryAction(action: string, input?: JsonObject): Promise<DirectoryActionOutput>;
  viewResource(resourceId: string, input?: JsonObject): Promise<ResourceActionOutput>;
  refreshDirectory(): Promise<void>;
  navigateToDirectory(path: string): Promise<void>;
  editResource(resourceId: string): Promise<void>;
  disconnect(): void;
}

export interface AssetHubResourceFrameMount {
  /** Resolves when the nested Resource frame has connected to its Directory-frame relay. */
  ready: Promise<void>;
  disconnect(): void;
}

export interface AssetHubFrameConnectionOptions {
  /** Maximum time to establish the frame connection. */
  connectionTimeoutMs?: number;
  /**
   * Maximum time to wait for each Host method response.
   *
   * A timeout stops waiting; it does not cancel Host work already in progress. In particular,
   * callers must not assume that a timed-out write failed or retry it blindly.
   */
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
    channel: RESOURCE_FRAME_CHANNEL,
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
    viewResource(resourceId, input) {
      return host.viewResource(
        resourceId,
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
    editResource(resourceId) {
      return host.editResource(resourceId, new CallOptions({ timeout: callTimeoutMs }));
    },
    disconnect() {
      connection.destroy();
    },
  };
}

/**
 * Mounts an existing read-only Resource plugin frame inside a Directory plugin frame.
 *
 * The nested frame receives the normal Resource-frame API. Calls are relayed through the
 * Directory-bound client, so the outer Host retains Resource membership and Action authority.
 */
export function mountAssetHubResourceFrame({
  client,
  frame,
  resourceId,
  output,
  connectionTimeoutMs,
}: {
  client: AssetHubDirectoryFrameClient;
  frame: HTMLIFrameElement;
  resourceId: string;
  output: ResourceActionOutput;
  connectionTimeoutMs?: number;
}): AssetHubResourceFrameMount {
  if (output.resourceId !== resourceId) {
    throw new Error("The Resource frame output is not bound to the requested Resource.");
  }
  const view = output.view;
  if (view?.view !== "plugin_frame") {
    throw new Error("The Resource Action did not return a plugin_frame.");
  }
  if (view.plugin_api !== PLUGIN_API_VERSION) {
    throw new Error(`Unsupported Plugin Frame API: ${view.plugin_api}`);
  }
  const source = pluginFrameSource(view.url);
  const remoteWindow = frame.contentWindow;
  if (!remoteWindow) throw new Error("The Resource frame window is not available.");

  frame.setAttribute("sandbox", "allow-scripts");
  frame.title = view.title ?? "Resource view";
  frame.src = source;

  const timeout = positiveTimeout(
    connectionTimeoutMs,
    defaultConnectionTimeoutMs,
    "connectionTimeoutMs",
  );
  const connection = connect<Record<string, never>>({
    messenger: new WindowMessenger({
      remoteWindow,
      allowedOrigins: ["*"],
    }),
    channel: RESOURCE_FRAME_CHANNEL,
    timeout,
    methods: {
      executeResourceAction(action: string, input?: JsonObject) {
        if (action !== output.action) {
          return Promise.reject(
            new Error("The nested Resource frame may execute only its originating Action."),
          );
        }
        return client.viewResource(resourceId, input ?? {});
      },
      replaceResourceText() {
        return Promise.reject(
          new Error("Text replacement is not available from an embedded read-only Resource frame."),
        );
      },
    } satisfies AssetHubFrameHost,
  });

  return {
    ready: connection.promise.then(() => undefined),
    disconnect() {
      connection.destroy();
    },
  };
}

function pluginFrameSource(value: string): string {
  const [path] = value.split(/[?#]/, 1);
  if (!path || !/^\/plugins\/[a-z0-9._-]+\//.test(path) || hasUnsafePathSegment(path)) {
    throw new Error("The Resource Action returned an invalid plugin frame URL.");
  }
  return value;
}

function hasUnsafePathSegment(path: string): boolean {
  try {
    return path.split("/").some((segment) => {
      const decoded = decodeURIComponent(segment);
      return decoded === "." || decoded === ".." || decoded.includes("/") || decoded.includes("\\");
    });
  } catch {
    return true;
  }
}

function positiveTimeout(value: number | undefined, fallback: number, name: string): number {
  if (value === undefined) return fallback;
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new TypeError(`${name} must be a positive safe integer.`);
  }
  return value;
}

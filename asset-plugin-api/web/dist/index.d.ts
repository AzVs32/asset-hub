export declare const PLUGIN_API_VERSION = "asset-hub.plugin-api@5";
export type JsonPrimitive = boolean | number | string | null;
export type JsonValue = JsonPrimitive | JsonValue[] | JsonObject;
export type JsonObject = {
    [key: string]: JsonValue;
};
export type PluginView = {
    view: "text";
    text: string;
} | {
    view: "markdown";
    markdown: string;
} | {
    view: "html";
    title?: string;
    html: string;
} | {
    view: "plugin_frame";
    plugin_api: string;
    title?: string;
    url: string;
} | {
    view: "json";
    data: unknown;
} | {
    view: "media";
    mime_type: string;
    title?: string;
    encoding: "base64" | "url";
    data: string;
} | {
    view: "download";
    url: string;
    mime_type?: string;
    filename?: string;
};
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
export interface AssetHubFrameClient {
    executeResourceAction(action: string, input?: JsonObject): Promise<ResourceActionOutput>;
    replaceResourceText(text: string): Promise<void>;
    disconnect(): void;
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
export declare function connectAssetHubFrame(options?: AssetHubFrameConnectionOptions): Promise<AssetHubFrameClient>;
/** Connects a Directory workspace iframe to capabilities bound to its current Directory. */
export declare function connectAssetHubDirectoryFrame(options?: AssetHubFrameConnectionOptions): Promise<AssetHubDirectoryFrameClient>;
//# sourceMappingURL=index.d.ts.map
import { type DirectoryActionOutput, type JsonObject, type ResourceActionOutput } from "./contract";
export * from "./contract";
export interface AssetHubFrameClient {
    executeResourceAction(action: string, input?: JsonObject): Promise<ResourceActionOutput>;
    replaceResourceText(text: string): Promise<void>;
    disconnect(): void;
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
export declare function connectAssetHubFrame(options?: AssetHubFrameConnectionOptions): Promise<AssetHubFrameClient>;
/** Connects a Directory workspace iframe to capabilities bound to its current Directory. */
export declare function connectAssetHubDirectoryFrame(options?: AssetHubFrameConnectionOptions): Promise<AssetHubDirectoryFrameClient>;
/**
 * Mounts an existing read-only Resource plugin frame inside a Directory plugin frame.
 *
 * The nested frame receives the normal Resource-frame API. Calls are relayed through the
 * Directory-bound client, so the outer Host retains Resource membership and Action authority.
 */
export declare function mountAssetHubResourceFrame({ client, frame, resourceId, output, connectionTimeoutMs, }: {
    client: AssetHubDirectoryFrameClient;
    frame: HTMLIFrameElement;
    resourceId: string;
    output: ResourceActionOutput;
    connectionTimeoutMs?: number;
}): AssetHubResourceFrameMount;

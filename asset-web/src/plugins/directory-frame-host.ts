import type { AssetGateway } from "@/application/ports/asset-gateway";
import { normalizeDirectory } from "@/domain/directory-path";
import type { DirectoryActionOutput, JsonObject } from "@/domain/plugin";
import type { Directory } from "@/domain/resource";

export const directoryPluginFrameChannel = "asset-hub.plugin-directory-frame@3";

export interface DirectoryPluginFrameHostMethods
  extends Record<string, (...args: never[]) => unknown> {
  executeDirectoryAction(action: unknown, input?: unknown): Promise<DirectoryActionOutput>;
  refreshDirectory(): Promise<void>;
  navigateToDirectory(path: unknown): Promise<void>;
}

export interface DirectoryPluginFrameHostBridge {
  methods: DirectoryPluginFrameHostMethods;
  updateDirectory(directory: Directory): void;
}

export function createDirectoryPluginFrameHostBridge({
  directory: initialDirectory,
  frameDirectoryId,
  gateway,
  onDirectoryChanged,
  onNavigate,
  confirmAction,
}: {
  directory: Directory;
  frameDirectoryId: string;
  gateway: AssetGateway;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
  confirmAction?: ((message: string) => boolean | Promise<boolean>) | undefined;
}): DirectoryPluginFrameHostBridge {
  let directory = initialDirectory;

  function boundDirectory(): Directory {
    if (frameDirectoryId !== directory.id) {
      throw new Error("The plugin frame is not bound to the current directory.");
    }
    return directory;
  }

  return {
    methods: {
      async executeDirectoryAction(actionValue, inputValue) {
        const current = boundDirectory();
        const actionId = parseActionId(actionValue);
        const input = parseInput(inputValue);
        const action = current.actions.find((candidate) => candidate.id === actionId);
        if (!action) throw new Error(`Action ${actionId} is not available.`);
        if (action.ui.confirmation) {
          const confirmed = await confirmAction?.(
            action.ui.confirmation.replaceAll("{name}", current.name),
          );
          if (!confirmed) throw new Error(`Action ${actionId} was not confirmed.`);
        }
        const result = await gateway.executeDirectoryAction(current, action, input);
        if (action.access === "write") await onDirectoryChanged?.();
        return result;
      },
      async refreshDirectory() {
        boundDirectory();
        await onDirectoryChanged?.();
      },
      async navigateToDirectory(pathValue) {
        boundDirectory();
        const path = parseDirectoryPath(pathValue);
        await onNavigate?.(path);
      },
    },
    updateDirectory(nextDirectory) {
      if (nextDirectory.id === directory.id && nextDirectory.revision >= directory.revision) {
        directory = nextDirectory;
      }
    },
  };
}

function parseActionId(value: unknown): string {
  if (typeof value !== "string" || !value || value.length > 128) {
    throw new TypeError("Action ID must be a non-empty string of at most 128 characters.");
  }
  return value;
}

function parseInput(value: unknown): JsonObject {
  if (value === undefined) return {};
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError("Action input must be a JSON object.");
  }
  return value as JsonObject;
}

function parseDirectoryPath(value: unknown): string {
  if (typeof value !== "string" || value.length > 4096 || normalizeDirectory(value) !== value) {
    throw new TypeError("Directory path must be a canonical relative path.");
  }
  return value;
}

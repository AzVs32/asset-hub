import type { AssetGateway } from "@/application/ports/asset-gateway";
import { normalizeDirectory } from "@/domain/directory-path";
import type { DirectoryActionOutput } from "@/domain/plugin";
import type { Directory } from "@/domain/resource";
import { parseActionId, parseActionInput } from "./frame-input";

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
        const input = parseActionInput(inputValue);
        const action = current.actions.find((candidate) => candidate.id === actionId);
        if (!action) throw new Error(`Action ${actionId} is not available.`);
        if (action.ui.confirmation) {
          const confirmed = await confirmAction?.(
            action.ui.confirmation.replaceAll("{name}", current.name),
          );
          if (!confirmed) throw new Error(`Action ${actionId} was not confirmed.`);
        }
        const result = await gateway.executeDirectoryAction(current, action.id, input);
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
      if (nextDirectory.id !== frameDirectoryId) {
        throw new Error("The plugin frame cannot change its bound Directory.");
      }
      if (nextDirectory.revision >= directory.revision) directory = nextDirectory;
    },
  };
}

function parseDirectoryPath(value: unknown): string {
  if (typeof value !== "string" || value.length > 4096 || normalizeDirectory(value) !== value) {
    throw new TypeError("Directory path must be a canonical relative path.");
  }
  return value;
}

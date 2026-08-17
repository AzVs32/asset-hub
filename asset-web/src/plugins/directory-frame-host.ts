import type { AssetGateway } from "@/application/ports/asset-gateway";
import { normalizeDirectory } from "@/domain/directory-path";
import type { DirectoryActionOutput, ResourceActionOutput } from "@/domain/plugin";
import type { Directory, Resource, ResourceAction } from "@/domain/resource";
import { parseActionId, parseActionInput, parseResourceId } from "./frame-input";

export interface DirectoryPluginFrameHostMethods
  extends Record<string, (...args: never[]) => unknown> {
  executeDirectoryAction(action: unknown, input?: unknown): Promise<DirectoryActionOutput>;
  viewResource(resourceId: unknown, input?: unknown): Promise<ResourceActionOutput>;
  refreshDirectory(): Promise<void>;
  navigateToDirectory(path: unknown): Promise<void>;
  editResource(resourceId: unknown): Promise<void>;
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
  onEditResource,
  confirmAction,
}: {
  directory: Directory;
  frameDirectoryId: string;
  gateway: AssetGateway;
  onDirectoryChanged?: (() => void | Promise<void>) | undefined;
  onNavigate?: ((path: string) => void | Promise<void>) | undefined;
  onEditResource?:
    | ((resource: Resource, action: ResourceAction) => void | Promise<void>)
    | undefined;
  confirmAction?: ((message: string) => boolean | Promise<boolean>) | undefined;
}): DirectoryPluginFrameHostBridge {
  let directory = initialDirectory;

  function boundDirectory(): Directory {
    if (frameDirectoryId !== directory.id) {
      throw new Error("The plugin frame is not bound to the current directory.");
    }
    return directory;
  }

  async function directResource(resourceIdValue: unknown): Promise<Resource> {
    const current = boundDirectory();
    const resourceId = parseResourceId(resourceIdValue);
    const resource = await gateway.findResource(resourceId);
    if (resource.directory !== current.path) {
      throw new Error("The Resource is not a direct member of the bound Directory.");
    }
    return resource;
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
      async viewResource(resourceIdValue, inputValue) {
        const resource = await directResource(resourceIdValue);
        const input = parseActionInput(inputValue);
        const viewer = resource.actions.find(
          (candidate) =>
            candidate.provides === "text_view" &&
            candidate.access === "read" &&
            candidate.output.effects.length === 0 &&
            candidate.output.views.includes("plugin_frame"),
        );
        if (!viewer) {
          throw new Error("No frame-based text viewer is available for this Resource.");
        }
        return gateway.executeResourceAction(resource, viewer.id, input);
      },
      async editResource(resourceIdValue) {
        if (!onEditResource) {
          throw new Error("Resource editing is not available from this Directory frame.");
        }
        const resource = await directResource(resourceIdValue);
        const editor = resource.actions.find(
          (candidate) =>
            candidate.provides === "text_edit" &&
            candidate.access === "write" &&
            candidate.output.views.includes("plugin_frame"),
        );
        if (!editor) {
          throw new Error("No frame-based text editor is available for this Resource.");
        }
        await onEditResource(resource, editor);
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

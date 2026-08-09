import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { JsonObject, ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";

export const pluginFrameApiVersion = "asset-hub.plugin-api@3";
export const pluginFrameChannel = "asset-hub.plugin-frame@3";

export interface PluginFrameHostMethods extends Record<string, (...args: never[]) => unknown> {
  executeResourceAction(action: unknown, input?: unknown): Promise<ResourceActionOutput>;
  replaceResourceText(text: unknown): Promise<void>;
}

export interface PluginFrameHostBridge {
  methods: PluginFrameHostMethods;
  updateResource(resource: Resource): void;
}

export function createPluginFrameHostBridge({
  resource: initialResource,
  frameResourceId,
  frameActionId,
  gateway,
  onResourceChanged,
}: {
  resource: Resource;
  frameResourceId: string;
  frameActionId: string;
  gateway: AssetGateway;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}): PluginFrameHostBridge {
  let resource = initialResource;

  function boundResource(): Resource {
    if (frameResourceId !== resource.id) {
      throw new Error("The plugin frame is not bound to the current resource.");
    }
    return resource;
  }

  return {
    methods: {
      async executeResourceAction(actionValue, inputValue) {
        const current = boundResource();
        const actionId = parseActionId(actionValue);
        const input = parseInput(inputValue);
        const action = current.actions.find((candidate) => candidate.id === actionId);
        if (!action) throw new Error(`Action ${actionId} is not available.`);
        const result = await gateway.executeAction(current, action.id, input);
        if (action.access === "write") await onResourceChanged?.();
        return result;
      },
      async replaceResourceText(textValue) {
        const current = boundResource();
        if (typeof textValue !== "string")
          throw new TypeError("Replacement text must be a string.");
        const editAction = current.actions.find(
          (candidate) =>
            candidate.id === frameActionId &&
            candidate.provides === "text_edit" &&
            candidate.access === "write",
        );
        if (!editAction) throw new Error("Text editing is not available from this frame.");
        resource = await gateway.replaceResourceText(current, textValue);
        await onResourceChanged?.();
      },
    },
    updateResource(nextResource) {
      if (nextResource.id !== resource.id || nextResource.revision > resource.revision) {
        resource = nextResource;
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

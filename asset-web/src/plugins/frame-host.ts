import type { AssetGateway } from "@/application/ports/asset-gateway";
import { RESOURCE_EDIT_CAPABILITY, type ResourceActionOutput } from "@/domain/plugin";
import type { Resource } from "@/domain/resource";
import { parseActionId, parseActionInput } from "./frame-input";

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
  confirmAction,
}: {
  resource: Resource;
  frameResourceId: string;
  frameActionId: string;
  gateway: AssetGateway;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
  confirmAction?: ((message: string) => boolean | Promise<boolean>) | undefined;
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
        const input = parseActionInput(inputValue);
        const action = current.actions.find((candidate) => candidate.id === actionId);
        if (!action) throw new Error(`Action ${actionId} is not available.`);
        if (action.ui.confirmation) {
          const confirmed = await confirmAction?.(
            action.ui.confirmation.replaceAll("{name}", current.name),
          );
          if (!confirmed) throw new Error(`Action ${actionId} was not confirmed.`);
        }
        const result = await gateway.executeResourceAction(current, action.id, input);
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
            candidate.provides === RESOURCE_EDIT_CAPABILITY &&
            candidate.access === "write",
        );
        if (!editAction) throw new Error("Text editing is not available from this frame.");
        resource = await gateway.replaceResourceText(current, textValue);
        await onResourceChanged?.();
      },
    },
    updateResource(nextResource) {
      if (nextResource.id !== frameResourceId) {
        throw new Error("The plugin frame cannot change its bound Resource.");
      }
      if (nextResource.revision >= resource.revision) resource = nextResource;
    },
  };
}

import React from "react";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { PluginActionOutput, PluginView, PluginViewKind } from "@/domain/plugin";
import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";
import { type HostSlot, hostSlots } from "./slots";

export interface PluginViewRendererProps {
  view: PluginView;
  output: PluginActionOutput;
  resource: Resource;
  gateway: AssetGateway;
  onResourceChanged?: (() => void | Promise<void>) | undefined;
}

export type PluginViewRenderer = React.ComponentType<PluginViewRendererProps>;

export class PluginKernel {
  readonly #viewRenderers = new Map<PluginViewKind, PluginViewRenderer>();

  registerView(kind: PluginViewKind, renderer: PluginViewRenderer): () => void {
    if (this.#viewRenderers.has(kind)) throw new Error(`View renderer already registered: ${kind}`);
    this.#viewRenderers.set(kind, renderer);
    return () => this.#viewRenderers.delete(kind);
  }

  viewRenderer(kind: PluginViewKind): PluginViewRenderer | null {
    return this.#viewRenderers.get(kind) ?? null;
  }

  actionsAt(resource: Resource, slot: HostSlot): ResourceAction[] {
    return sortActions(
      resource.actions.filter((action) => {
        if (action.ui.locations.includes(slot)) return true;
        if (slot !== hostSlots.resourceDetailActions) return false;
        return (
          action.ui.locations.length === 0 ||
          action.ui.locations.every((location) => !knownSlots.has(location as HostSlot))
        );
      }),
    );
  }

  thumbnailAction(resource: Resource): ResourceAction | null {
    return (
      this.actionsAt(resource, hostSlots.resourceListThumbnail).find(
        (action) => action.access === "read_only",
      ) ?? null
    );
  }

  directoryActionsAt(directory: Directory, slot: HostSlot): DirectoryAction[] {
    return sortActions(
      directory.actions.filter((action) => {
        if (action.ui.locations.includes(slot)) return true;
        if (slot !== hostSlots.directoryToolbar) return false;
        return (
          action.ui.locations.length === 0 ||
          action.ui.locations.every((location) => !knownSlots.has(location as HostSlot))
        );
      }),
    );
  }
}

const knownSlots = new Set<HostSlot>(Object.values(hostSlots));

function sortActions<T extends ResourceAction | DirectoryAction>(actions: T[]): T[] {
  return [...actions].sort(
    (left, right) =>
      (left.ui.group ?? "").localeCompare(right.ui.group ?? "") ||
      (left.ui.order ?? 0) - (right.ui.order ?? 0) ||
      left.label.localeCompare(right.label),
  );
}

const PluginKernelContext = React.createContext<PluginKernel | null>(null);

export function PluginKernelProvider({
  kernel,
  children,
}: {
  kernel: PluginKernel;
  children: React.ReactNode;
}) {
  return <PluginKernelContext.Provider value={kernel}>{children}</PluginKernelContext.Provider>;
}

export function usePluginKernel(): PluginKernel {
  const kernel = React.useContext(PluginKernelContext);
  if (!kernel) throw new Error("PluginKernelProvider is missing");
  return kernel;
}

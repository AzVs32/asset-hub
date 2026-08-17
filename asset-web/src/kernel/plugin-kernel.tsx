import React from "react";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import {
  DIRECTORY_THUMBNAIL_CAPABILITY,
  DIRECTORY_WORKSPACE_CAPABILITY,
  type PluginView,
  type PluginViewKind,
  RESOURCE_THUMBNAIL_CAPABILITY,
  type ResourceActionOutput,
} from "@/domain/plugin";
import type { Directory, DirectoryAction, Resource, ResourceAction } from "@/domain/resource";
import {
  type CoreDirectoryWorkspaceSlot,
  coreDirectoryWorkspaceSlots,
  directoryWorkspaceOutlet,
} from "./slots";

export interface PluginViewRendererProps {
  view: PluginView;
  output: ResourceActionOutput;
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

  resourceActionsAtCoreSlot(
    resource: Resource,
    slot: CoreDirectoryWorkspaceSlot,
  ): ResourceAction[] {
    return sortActions(
      resource.actions.filter((action) => {
        if (action.ui.locations.includes(slot)) return true;
        if (slot !== coreDirectoryWorkspaceSlots.resourceContextMenu) return false;
        return (
          action.ui.locations.length === 0 ||
          action.ui.locations.every((location) => !knownHostLocations.has(location))
        );
      }),
    );
  }

  thumbnailAction(resource: Resource): ResourceAction | null {
    return (
      this.resourceActionsAtCoreSlot(resource, coreDirectoryWorkspaceSlots.resourceThumbnail).find(
        (action) => action.access === "read" && action.provides === RESOURCE_THUMBNAIL_CAPABILITY,
      ) ?? null
    );
  }

  directoryActionsAtCoreSlot(
    directory: Directory,
    slot: CoreDirectoryWorkspaceSlot,
  ): DirectoryAction[] {
    return sortActions(
      directory.actions.filter((action) => {
        if (action.ui.locations.includes(slot)) return true;
        if (slot !== coreDirectoryWorkspaceSlots.directoryContextMenu) return false;
        return (
          action.ui.locations.length === 0 ||
          action.ui.locations.every((location) => !knownHostLocations.has(location))
        );
      }),
    );
  }

  directoryThumbnailAction(directory: Directory): DirectoryAction | null {
    return (
      this.directoryActionsAtCoreSlot(
        directory,
        coreDirectoryWorkspaceSlots.directoryThumbnail,
      ).find(
        (action) => action.access === "read" && action.provides === DIRECTORY_THUMBNAIL_CAPABILITY,
      ) ?? null
    );
  }

  directoryWorkspaceAction(directory: Directory): DirectoryAction | null {
    return (
      directory.actions.find(
        (action) =>
          action.provides === DIRECTORY_WORKSPACE_CAPABILITY &&
          action.access === "read" &&
          action.output.views.includes("plugin_frame") &&
          action.output.effects.length === 0 &&
          action.ui.locations.includes(directoryWorkspaceOutlet),
      ) ?? null
    );
  }
}

const knownHostLocations = new Set<string>([
  directoryWorkspaceOutlet,
  ...Object.values(coreDirectoryWorkspaceSlots),
]);

function sortActions<T extends ResourceAction | DirectoryAction>(actions: T[]): T[] {
  return [...actions].sort(
    (left, right) =>
      Number(left.ui.destructive) - Number(right.ui.destructive) ||
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

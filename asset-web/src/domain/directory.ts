import type { ActionAccess, ActionUi, DefinitionOrigin } from "./action";
import type {
  DirectoryActionCapabilityId,
  DirectoryActionEffectKind,
  PluginViewKind,
} from "./plugin";
import type { ResourcePage } from "./resource";

export type DirectoryResourceAccess = "none" | "metadata" | "content";

export interface DirectoryAction {
  id: string;
  origin: DefinitionOrigin;
  provides: DirectoryActionCapabilityId | null;
  label: string;
  description: string | null;
  access: ActionAccess;
  requires: { children: boolean; resources: DirectoryResourceAccess };
  output: { views: PluginViewKind[]; effects: DirectoryActionEffectKind[] };
  ui: ActionUi;
  appliesTo: { kinds: string[] };
}

export interface Directory {
  id: string;
  parentId: string | null;
  path: string;
  parentPath: string;
  name: string;
  kind: string;
  actions: DirectoryAction[];
  createdAt: string;
  updatedAt: string;
  revision: number;
}

export interface DirectoryKind {
  kind: string;
  parent: string | null;
  ancestors: string[];
  allowedParentKinds: string[];
  label: string;
  origin: DefinitionOrigin;
  actions: DirectoryAction[];
}

export interface DirectoryListing {
  path: string;
  directory: Directory;
  folders: Directory[];
  resources: ResourcePage;
}

export interface DirectoryPatch {
  name?: string;
  parentId?: string;
  kind?: string;
}

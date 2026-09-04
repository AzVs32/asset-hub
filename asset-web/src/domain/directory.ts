import type { DefinitionOrigin } from "./definition";
import type { ResourcePage } from "./resource";

export interface Directory {
  id: string;
  parentId: string | null;
  path: string;
  parentPath: string;
  name: string;
  kind: string;
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

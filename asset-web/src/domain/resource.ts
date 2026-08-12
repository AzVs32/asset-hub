import type { PluginViewKind } from "./plugin";

export type ActionAccess = "read" | "write";
export type ContentDelivery = "auto" | "inline" | "reference";
export type ResourceActionEffectKind = "replace_content" | "delete";
export type DirectoryActionEffectKind = "update" | "create_child" | "delete";
export interface ActionUi {
  group: string | null;
  order: number | null;
  locations: string[];
  destructive: boolean;
  confirmation: string | null;
}
export interface DefinitionOrigin {
  kind: "builtin" | "plugin";
  id: string;
}

export interface ResourceContent {
  size: number;
  mimeType: string | null;
  verificationStatus: "pending" | "verified" | "failed";
  checksum: { kind: string; value: string } | null;
  verificationError: string | null;
}

export interface ResourceAction {
  id: string;
  origin: DefinitionOrigin;
  provides: string | null;
  label: string;
  description: string | null;
  access: ActionAccess;
  requires: { content: boolean; contentDelivery: ContentDelivery };
  output: { views: PluginViewKind[]; effects: ResourceActionEffectKind[] };
  ui: ActionUi;
  appliesTo: { kinds: string[]; mimeTypes: string[]; extensions: string[] };
}

export interface DirectoryAction {
  id: string;
  origin: DefinitionOrigin;
  provides: string | null;
  label: string;
  description: string | null;
  access: ActionAccess;
  requires: { children: boolean; resources: boolean };
  output: { views: PluginViewKind[]; effects: DirectoryActionEffectKind[] };
  ui: ActionUi;
  appliesTo: { kinds: string[] };
}

export interface Resource {
  id: string;
  name: string;
  directory: string;
  kind: string;
  content: ResourceContent | null;
  actions: ResourceAction[];
  createdAt: string;
  updatedAt: string;
  revision: number;
  deletedAt: string | null;
}

export interface ResourceKind {
  kind: string;
  parent: string | null;
  ancestors: string[];
  label: string;
  supportsContent: boolean;
  origin: DefinitionOrigin;
  actions: ResourceAction[];
  detect: { mimeTypes: string[]; extensions: string[] } | null;
}

export interface ResourcePage {
  items: Resource[];
  total: number;
  page: number;
  limit: number;
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

export interface ResourceFilters {
  directory: string;
  page: number;
  limit: number;
  query: string;
  kind: string;
  includeDeleted: boolean;
}

export interface ResourceDraft {
  name: string;
  directory: string;
  kind: string;
}

export interface DirectoryDraft {
  name: string;
  parentId: string;
  kind: string;
}

export interface UploadDraft {
  file: File;
  name: string;
  directory: string;
  kind: string;
}

export interface UploadProgress {
  stage: "preparing" | "uploading" | "finalizing";
  bytesSent: number;
  totalBytes: number;
}

export interface UploadReceipt {
  id: string;
  name: string;
}

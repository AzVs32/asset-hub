import type { PluginViewKind } from "./plugin";

export type ResourceStatus = "active" | "archived";
export type ActionAccess = "read_only" | "read_write";
export type ActionExecutor = "builtin" | "plugin";
export type ContentDelivery = "auto" | "inline" | "reference";

export type JsonSchema = Record<string, unknown>;

export interface ResourceKindMetadataLayer {
  kind: string;
  schemaVersion: number;
  data: Record<string, unknown>;
}

export interface ResourceKindMetadataSet {
  layers: ResourceKindMetadataLayer[];
}

export interface ResourceKindMetadataDefinition {
  schemaVersion: number;
  schema: JsonSchema;
}

export interface ResourceKindMetadataPatch {
  upsert: ResourceKindMetadataLayer[];
  clear: string[];
}

export interface ResourceMetadata {
  summary: {
    description: string | null;
    tags: string[];
  };
  kindMetadata: ResourceKindMetadataSet;
}

export interface ResourceContent {
  key: string;
  size: number;
  mimeType: string | null;
  originalFilename: string | null;
  checksums: Array<{ kind: string; value: string }>;
}

export interface ResourceAction {
  id: string;
  label: string;
  description: string | null;
  access: ActionAccess;
  executor: { type: ActionExecutor; handler: string | null };
  requires: { content: boolean; contentDelivery: ContentDelivery };
  output: { views: PluginViewKind[] };
  ui: { group: string | null; order: number | null; locations: string[] };
  appliesTo: { kinds: string[]; mimeTypes: string[]; extensions: string[] };
}

export interface Resource {
  id: string;
  name: string;
  directory: string;
  kind: string;
  status: ResourceStatus;
  metadata: ResourceMetadata;
  content: ResourceContent | null;
  actions: ResourceAction[];
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
}

export interface ResourceKind {
  kind: string;
  parent: string | null;
  ancestors: string[];
  label: string;
  supportsContent: boolean;
  source: string;
  actions: ResourceAction[];
  detect: { mimeTypes: string[]; extensions: string[] } | null;
  metadata: ResourceKindMetadataDefinition | null;
}

export interface ResourcePage {
  items: Resource[];
  total: number;
  page: number;
  limit: number;
}

export interface Directory {
  path: string;
  parentPath: string;
  name: string;
}

export interface DirectoryListing {
  path: string;
  folders: Directory[];
  resources: ResourcePage;
}

export interface ResourceFilters {
  directory: string;
  page: number;
  limit: number;
  query: string;
  tag: string;
  kind: string;
  includeDescendants: boolean;
  includeDeleted: boolean;
}

export interface ResourceDraft {
  name: string;
  directory: string;
  kind: string;
  status: ResourceStatus;
  description: string;
  tags: string;
}

export interface UploadDraft {
  file: File;
  name: string;
  directory: string;
  kind: string;
  description: string;
  tags: string;
}

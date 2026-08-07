import type { PluginViewKind } from "./plugin";

export type ActionAccess = "read_only" | "read_write";
export type ContentDelivery = "auto" | "inline" | "reference";

export interface ResourceContent {
  size: number;
  mimeType: string | null;
  verificationStatus: "pending" | "verified" | "failed";
  checksum: { kind: string; value: string } | null;
  verificationError: string | null;
}

export interface ResourceAction {
  id: string;
  provides: string | null;
  label: string;
  description: string | null;
  access: ActionAccess;
  requires: { content: boolean; contentDelivery: ContentDelivery };
  output: { views: PluginViewKind[] };
  ui: { group: string | null; order: number | null; locations: string[] };
  appliesTo: { kinds: string[]; mimeTypes: string[]; extensions: string[] };
}

export interface DirectoryAction {
  id: string;
  provides: string | null;
  label: string;
  description: string | null;
  access: ActionAccess;
  requires: { children: boolean; resources: boolean };
  output: { views: PluginViewKind[] };
  ui: { group: string | null; order: number | null; locations: string[] };
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
  source: string;
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
  path: string;
  parentPath: string;
  name: string;
  kind: string;
  actions: DirectoryAction[];
}

export interface DirectoryKind {
  kind: string;
  parent: string | null;
  ancestors: string[];
  label: string;
  source: string;
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

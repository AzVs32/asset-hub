import type { ActionAccess, ActionUi, DefinitionOrigin } from "./action";
import type {
  PluginViewKind,
  ResourceActionCapabilityId,
  ResourceActionEffectKind,
} from "./plugin";

export type { ResourceActionEffectKind } from "./plugin";

export type ResourceContentDelivery = "auto" | "inline" | "reference";

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
  provides: ResourceActionCapabilityId | null;
  label: string;
  description: string | null;
  access: ActionAccess;
  requires: { content: boolean; contentDelivery: ResourceContentDelivery };
  output: { views: PluginViewKind[]; effects: ResourceActionEffectKind[] };
  ui: ActionUi;
  appliesTo: { kinds: string[]; mimeTypes: string[]; extensions: string[] };
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

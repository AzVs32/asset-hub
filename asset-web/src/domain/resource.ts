export type ResourceContentDelivery = "auto" | "inline" | "reference";
export type ResourceContentState = "absent" | "pending" | "verified" | "failed";
export type ResourceEffectiveState =
  | "deleted"
  | "no_content"
  | "verifying"
  | "ready"
  | "verification_failed";
export type ResourceLifecycleState = { status: "active" } | { status: "deleted"; at: string };

export interface ResourceState {
  lifecycle: ResourceLifecycleState;
  content: ResourceContentState;
  effective: ResourceEffectiveState;
}

export interface ResourceContent {
  size: number;
  mimeType: string | null;
  checksum: { kind: string; value: string } | null;
  verificationError: string | null;
}

export interface Resource {
  id: string;
  name: string;
  directoryId: string;
  directory: string;
  state: ResourceState;
  content: ResourceContent | null;
  createdAt: string;
  updatedAt: string;
  revision: number;
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
}

export interface ResourceDraft {
  name: string;
  directory: string;
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

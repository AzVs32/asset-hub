import type { Resource } from "@/domain/resource";

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

export type UploadPublication =
  | { status: "uploading" | "finalizing" }
  | { status: "failed"; message: string }
  | { status: "completed"; resource: Resource };

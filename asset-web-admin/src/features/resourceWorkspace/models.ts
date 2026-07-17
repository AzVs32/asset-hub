import type { ResourceStatus } from "../../api/contracts";

export type Filters = {
  q: string;
  kind: string;
  tag: string;
  includeDeleted: boolean;
  includeDescendants: boolean;
  page: number;
  limit: number;
};

export type Draft = {
  name: string;
  directory: string;
  kind: string;
  status: ResourceStatus;
  description: string;
  tags: string;
};

export type UploadDraft = {
  file: File | null;
  name: string;
  kind: string;
  directory: string;
  description: string;
  tags: string;
};


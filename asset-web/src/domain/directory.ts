import type { ResourcePage } from "./resource";

export interface Directory {
  id: string;
  parentId: string | null;
  path: string;
  parentPath: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  revision: number;
}

export interface DirectoryListing {
  path: string;
  directory: Directory;
  folders: Directory[];
  resources: ResourcePage;
}

export interface DirectoryListingQuery {
  directory: string;
  page: number;
  limit: number;
}

import type { DirectoryListingQuery } from "@/domain/directory";

export const queryKeys = {
  directory: (filters: DirectoryListingQuery) => ["directory", filters] as const,
  resource: (id: string) => ["resource", id] as const,
  upload: (id: string) => ["upload", id] as const,
};

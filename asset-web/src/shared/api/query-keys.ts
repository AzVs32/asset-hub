import type { ResourceFilters } from "@/domain/resource";

export const queryKeys = {
  directory: (filters: ResourceFilters) => ["directory", filters] as const,
  resource: (id: string) => ["resource", id] as const,
};

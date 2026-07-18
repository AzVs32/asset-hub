import type { ResourceFilters } from "@/domain/resource";

export const queryKeys = {
  session: ["session"] as const,
  resourceKinds: ["resource-kinds"] as const,
  directory: (filters: ResourceFilters) => ["directory", filters] as const,
  resource: (id: string) => ["resource", id] as const,
  users: ["users"] as const,
};

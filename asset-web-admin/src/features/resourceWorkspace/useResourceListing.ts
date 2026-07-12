import React from "react";
import { request } from "../../api";
import type { DirectoryListing, Filters, Resource, ResourceDirectory, ResourceKindOption, ResourceKindsResponse, ResourcePage } from "../../types";
import { errorMessage, sortKindsForHierarchy } from "../../utils/resourceDrafts";

const defaultFilters: Filters = {
  q: "", kind: "", tag: "", includeDeleted: false,
  includeDescendants: true, page: 1, limit: 20,
};

const fallbackKinds: ResourceKindOption[] = [{
  kind: "core:unknown", parent: null, ancestors: [], label: "core:unknown",
  schema_id: null, metadata_schema: null, supports_content: true,
  detect: undefined, actions: [], source: "builtin",
}];

export function useResourceListing(initialDirectory: string) {
  const [filters, setFilters] = React.useState(defaultFilters);
  const [page, setPage] = React.useState<ResourcePage>({ items: [], total: 0, page: 1, limit: defaultFilters.limit });
  const [currentDirectory, setCurrentDirectory] = React.useState(initialDirectory);
  const [folders, setFolders] = React.useState<ResourceDirectory[]>([]);
  const [resourceKinds, setResourceKinds] = React.useState(fallbackKinds);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    request<ResourceKindsResponse>("/resource-kinds")
      .then((result) => result.items.length && setResourceKinds(sortKindsForHierarchy(result.items)))
      .catch((reason) => setError(errorMessage(reason)));
  }, []);

  const reload = React.useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const params = new URLSearchParams({ page: String(filters.page), limit: String(filters.limit), path: currentDirectory });
      if (filters.q.trim()) params.set("q", filters.q.trim());
      if (filters.kind.trim()) params.set("kind", filters.kind.trim());
      if (filters.tag.trim()) params.set("tag", filters.tag.trim());
      if (filters.includeDeleted) params.set("include_deleted", "true");
      if (filters.kind && filters.includeDescendants) params.set("include_descendants", "true");
      const result = await request<DirectoryListing>(`/directories?${params.toString()}`);
      setFolders(result.folders);
      setPage(result.resources);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, [currentDirectory, filters]);

  React.useEffect(() => { void reload(); }, [reload]);

  function updateFilters(patch: Partial<Filters>) {
    setFilters((current) => ({ ...current, ...patch, page: patch.page ?? 1 }));
  }

  function openDirectory(path: string) {
    setCurrentDirectory(path);
    setFilters((current) => ({ ...current, page: 1 }));
  }

  function replaceResource(resource: Resource) {
    setPage((current) => ({
      ...current,
      items: current.items.map((item) => item.id === resource.id ? resource : item),
    }));
  }

  return { filters, setFilters, updateFilters, page, folders, currentDirectory, openDirectory,
    resourceKinds, contentKinds: resourceKinds.filter((kind) => kind.supports_content),
    loading, error, setError, reload, replaceResource };
}

import { useQuery } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import type { ResourceFilters } from "@/domain/resource";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import { decodeDirectoryPath, directoryPath } from "@/shared/routing/paths";

export function useAssetWorkspaceListing() {
  const gateway = useAssetWorkspaceGateway();
  const navigate = useNavigate();
  const route = useParams<"*">();
  const [searchParams, setSearchParams] = useSearchParams();
  const routeDirectory = decodeDirectoryPath(route["*"] ?? "");
  const filters = useMemo<ResourceFilters>(
    () => ({
      directory: routeDirectory,
      page: positiveInteger(searchParams.get("page"), 1),
      limit: 30,
    }),
    [routeDirectory, searchParams],
  );

  const listing = useQuery({
    queryKey: queryKeys.directory(filters),
    queryFn: ({ signal }) => gateway.listDirectory(filters, signal),
    placeholderData: (previous) => previous,
    refetchInterval: (query) =>
      query.state.data?.resources.items.some((resource) => resource.state.content === "pending")
        ? 1_000
        : false,
  });
  const updateFilters = useCallback(
    (patch: Partial<ResourceFilters>) => {
      setSearchParams((current) => searchParamsForFilters(current, filters, patch), {
        replace: true,
      });
    },
    [filters, setSearchParams],
  );

  const selectResource = useCallback(
    (id: string | null) => {
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current);
          setOrDelete(next, "resource", id ?? "");
          if (id) next.delete("folder");
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  const selectDirectory = useCallback(
    (id: string | null) => {
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current);
          setOrDelete(next, "folder", id ?? "");
          if (id) next.delete("resource");
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  const openDirectory = useCallback(
    (directory: string) => {
      const next = new URLSearchParams(searchParams);
      next.delete("page");
      next.delete("resource");
      next.delete("folder");
      navigate({ pathname: directoryPath(directory), search: next.toString() });
    },
    [navigate, searchParams],
  );

  return {
    filters,
    updateFilters,
    openDirectory,
    selectResource,
    selectDirectory,
    selectedId: searchParams.get("resource"),
    selectedDirectoryId: searchParams.get("folder"),
    listing,
  };
}

function searchParamsForFilters(
  current: URLSearchParams,
  filters: ResourceFilters,
  patch: Partial<ResourceFilters>,
): URLSearchParams {
  const next = new URLSearchParams(current);
  const merged = { ...filters, ...patch };
  setOrDelete(next, "page", merged.page === 1 ? "" : String(merged.page));
  next.delete("resource");
  next.delete("folder");
  return next;
}

function setOrDelete(params: URLSearchParams, key: string, value: string) {
  if (value) params.set(key, value);
  else params.delete(key);
}

function positiveInteger(value: string | null, fallback: number): number {
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

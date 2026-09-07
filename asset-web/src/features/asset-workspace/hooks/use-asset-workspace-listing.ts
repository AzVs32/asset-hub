import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo } from "react";
import { useLocation, useNavigate, useSearchParams } from "react-router";
import type { DirectoryListingQuery } from "@/domain/directory";

import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import { decodeDirectoryPath, directoryPath } from "@/shared/routing/paths";

export function useAssetWorkspaceListing() {
  const gateway = useAssetWorkspaceGateway();
  const navigate = useNavigate();
  const location = useLocation();
  const [searchParams, setSearchParams] = useSearchParams();
  // Router params are already decoded; decode the original URL exactly once.
  const routeDirectory = decodeDirectoryPath(location.pathname);
  const filters = useMemo<DirectoryListingQuery>(
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
    refetchInterval: (query) =>
      query.state.data?.resources.items.some((resource) => resource.state.content === "pending")
        ? 1_000
        : false,
  });
  const updateFilters = useCallback(
    (page: number) => {
      setSearchParams((current) => searchParamsForPage(current, page), {
        replace: true,
      });
    },
    [setSearchParams],
  );

  useEffect(() => {
    if (!listing.isSuccess || listing.isFetching) return;
    const lastPage = Math.max(1, Math.ceil(listing.data.resources.total / filters.limit));
    if (filters.page > lastPage) updateFilters(lastPage);
  }, [
    listing.isSuccess,
    listing.isFetching,
    listing.data,
    filters.limit,
    filters.page,
    updateFilters,
  ]);

  const selectedId = searchParams.get("resource") || null;
  const selectedDirectoryId = selectedId ? null : searchParams.get("folder") || null;
  useEffect(() => {
    if (!selectedId || !searchParams.has("folder")) return;
    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.delete("folder");
        return next;
      },
      { replace: true },
    );
  }, [selectedId, searchParams, setSearchParams]);

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

  const clearSelection = useCallback(
    (kind: "resource" | "folder", id: string) => {
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current);
          if (next.get(kind) === id) next.delete(kind);
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  return {
    filters,
    updateFilters,
    openDirectory,
    selectResource,
    selectDirectory,
    clearSelection,
    selectedId,
    selectedDirectoryId,
    listing,
  };
}

function searchParamsForPage(current: URLSearchParams, page: number): URLSearchParams {
  const next = new URLSearchParams(current);
  setOrDelete(next, "page", page === 1 ? "" : String(page));
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
  return Number.isSafeInteger(parsed) && parsed > 0 ? parsed : fallback;
}

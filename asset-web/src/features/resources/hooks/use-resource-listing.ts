import { useQuery } from "@tanstack/react-query";
import { useCallback, useMemo } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { decodeDirectoryPath, directoryPath } from "@/app/paths";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { ResourceFilters } from "@/domain/resource";

export function useResourceListing() {
  const gateway = useGateway();
  const navigate = useNavigate();
  const route = useParams<"*">();
  const [searchParams, setSearchParams] = useSearchParams();
  const routeDirectory = decodeDirectoryPath(route["*"] ?? "");
  const filters = useMemo<ResourceFilters>(
    () => ({
      directory: routeDirectory,
      page: positiveInteger(searchParams.get("page"), 1),
      limit: 30,
      query: searchParams.get("q") ?? "",
      tag: searchParams.get("tag") ?? "",
      kind: searchParams.get("kind") ?? "",
      includeDescendants: searchParams.get("descendants") === "1",
      includeDeleted: searchParams.get("deleted") === "1",
    }),
    [routeDirectory, searchParams],
  );

  const kinds = useQuery({
    queryKey: queryKeys.resourceKinds,
    queryFn: () => gateway.listResourceKinds(),
    staleTime: 5 * 60_000,
  });
  const directoryKinds = useQuery({
    queryKey: queryKeys.directoryKinds,
    queryFn: () => gateway.listDirectoryKinds(),
    staleTime: 5 * 60_000,
  });
  const listing = useQuery({
    queryKey: queryKeys.directory(filters),
    queryFn: ({ signal }) => gateway.listDirectory(filters, signal),
    placeholderData: (previous) => previous,
  });
  const updateFilters = useCallback(
    (patch: Partial<ResourceFilters>) => {
      setSearchParams(
        (current) => {
          const next = new URLSearchParams(current);
          const merged = { ...filters, ...patch };
          setOrDelete(next, "q", merged.query);
          setOrDelete(next, "tag", merged.tag);
          setOrDelete(next, "kind", merged.kind);
          setOrDelete(next, "page", merged.page === 1 ? "" : String(merged.page));
          setOrDelete(next, "descendants", merged.includeDescendants ? "1" : "");
          setOrDelete(next, "deleted", merged.includeDeleted ? "1" : "");
          return next;
        },
        { replace: true },
      );
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
    kinds,
    directoryKinds,
  };
}

function setOrDelete(params: URLSearchParams, key: string, value: string) {
  if (value) params.set(key, value);
  else params.delete(key);
}

function positiveInteger(value: string | null, fallback: number): number {
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

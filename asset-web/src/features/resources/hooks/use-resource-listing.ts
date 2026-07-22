import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { canonicalDirectoryRoute, decodeDirectoryPath, directoryPath } from "@/app/paths";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { WorkspaceResourceFilters } from "@/application/workspace/workspace-scope";
import { useWorkspaceScope } from "@/application/workspace/workspace-scope-context";
import { visibleDirectory } from "@/domain/directory-path";

export function useResourceListing() {
  const gateway = useGateway();
  const scope = useWorkspaceScope();
  const navigate = useNavigate();
  const route = useParams<"*">();
  const [searchParams, setSearchParams] = useSearchParams();
  const routeDirectory = decodeDirectoryPath(route["*"] ?? "");
  const canonicalRoute = canonicalDirectoryRoute(routeDirectory, scope);
  const filters = useMemo<WorkspaceResourceFilters>(
    () => ({
      directory: canonicalRoute.directory,
      page: positiveInteger(searchParams.get("page"), 1),
      limit: 30,
      query: searchParams.get("q") ?? "",
      tag: searchParams.get("tag") ?? "",
      kind: searchParams.get("kind") ?? "",
      includeDescendants: searchParams.get("descendants") === "1",
      includeDeleted: searchParams.get("deleted") === "1",
    }),
    [canonicalRoute.directory, searchParams],
  );
  const storageFilters = useMemo(() => scope.toStorageFilters(filters), [filters, scope]);

  useEffect(() => {
    if (canonicalRoute.changed) {
      navigate(
        {
          pathname: directoryPath(canonicalRoute.directory),
          search: searchParams.toString(),
        },
        { replace: true },
      );
    }
  }, [canonicalRoute.changed, canonicalRoute.directory, navigate, searchParams]);

  const kinds = useQuery({
    queryKey: queryKeys.resourceKinds,
    queryFn: () => gateway.listResourceKinds(),
    staleTime: 5 * 60_000,
  });
  const listing = useQuery({
    queryKey: queryKeys.directory(storageFilters),
    queryFn: async ({ signal }) =>
      scope.toVisibleListing(await gateway.listDirectory(storageFilters, signal)),
    placeholderData: (previous) => previous,
  });
  const updateFilters = useCallback(
    (patch: Partial<WorkspaceResourceFilters>) => {
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
      navigate({ pathname: directoryPath(visibleDirectory(directory)), search: next.toString() });
    },
    [navigate, searchParams],
  );

  return {
    filters,
    updateFilters,
    openDirectory,
    selectResource,
    selectedId: searchParams.get("resource"),
    listing,
    kinds,
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

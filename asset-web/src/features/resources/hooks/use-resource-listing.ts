import { useQuery } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { directoryPath } from "@/app/paths";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { ResourceFilters } from "@/domain/resource";
import { useSession } from "@/features/auth/session-context";

export function useResourceListing() {
  const gateway = useGateway();
  const user = useSession();
  const navigate = useNavigate();
  const route = useParams<"*">();
  const [searchParams, setSearchParams] = useSearchParams();
  const routeDirectory = route["*"] ?? "";
  const filters = useMemo<ResourceFilters>(
    () => ({
      directory: routeDirectory || (user.isAdmin ? "" : user.workspaceDirectory),
      page: positiveInteger(searchParams.get("page"), 1),
      limit: 30,
      query: searchParams.get("q") ?? "",
      tag: searchParams.get("tag") ?? "",
      kind: searchParams.get("kind") ?? "",
      includeDescendants: searchParams.get("descendants") === "1",
      includeDeleted: searchParams.get("deleted") === "1",
    }),
    [routeDirectory, searchParams, user.isAdmin, user.workspaceDirectory],
  );

  useEffect(() => {
    if (!routeDirectory && !user.isAdmin && user.workspaceDirectory) {
      navigate(
        { pathname: directoryPath(user.workspaceDirectory), search: searchParams.toString() },
        { replace: true },
      );
    }
  }, [navigate, routeDirectory, searchParams, user.isAdmin, user.workspaceDirectory]);

  const kinds = useQuery({
    queryKey: queryKeys.resourceKinds,
    queryFn: () => gateway.listResourceKinds(),
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
      navigate({ pathname: directoryPath(directory), search: next.toString() });
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

import type { QueryClient } from "@tanstack/react-query";
import type { AssetWorkspaceGateway } from "@/shared/api/gateways";
import { queryKeys } from "@/shared/api/query-keys";

export function refreshDirectories(client: QueryClient, paths: readonly string[]) {
  return client.invalidateQueries({
    queryKey: ["directory"],
    predicate: ({ queryKey }) => {
      const filters = queryKey[1];
      return (
        typeof filters === "object" &&
        filters !== null &&
        "directory" in filters &&
        typeof filters.directory === "string" &&
        paths.includes(filters.directory)
      );
    },
  });
}

export async function refreshResource(
  client: QueryClient,
  gateway: Pick<AssetWorkspaceGateway, "findResource">,
  id: string,
) {
  const queryKey = queryKeys.resource(id);
  // A request started before the conflict must not overwrite the new snapshot.
  await client.cancelQueries({ queryKey });
  return client.fetchQuery({
    queryKey,
    queryFn: ({ signal }) => gateway.findResource(id, signal),
    staleTime: 0,
  });
}

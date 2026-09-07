import { useQuery } from "@tanstack/react-query";
import { useAssetWorkspaceGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";

export function useResourceDetail(id: string | null) {
  const gateway = useAssetWorkspaceGateway();
  return useQuery({
    queryKey: queryKeys.resource(id ?? ""),
    queryFn: ({ signal }) => gateway.findResource(id ?? "", signal),
    enabled: Boolean(id),
    refetchInterval: (query) => (query.state.data?.state.content === "pending" ? 1_000 : false),
  });
}

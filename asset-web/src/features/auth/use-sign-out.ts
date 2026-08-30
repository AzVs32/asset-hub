import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import type { CurrentUser } from "@/domain/auth";
import { useAuthGateway } from "@/shared/api/gateway-context";
import { queryKeys } from "@/shared/api/query-keys";
import { LOGIN_PATH } from "@/shared/routing/paths";

export function useSignOut(): () => Promise<void> {
  const gateway = useAuthGateway();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  return async () => {
    await gateway.logout();
    queryClient.setQueryData<CurrentUser | null>(queryKeys.session, null);
    queryClient.removeQueries({
      predicate: (query) => query.queryKey[0] !== queryKeys.session[0],
    });
    navigate(LOGIN_PATH, { replace: true });
  };
}

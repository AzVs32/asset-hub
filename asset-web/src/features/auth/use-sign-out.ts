import { useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { LOGIN_PATH } from "@/app/paths";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { CurrentUser } from "@/domain/auth";

export function useSignOut(): () => Promise<void> {
  const gateway = useGateway();
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

import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { AuthenticationRequiredError } from "@/application/errors";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import { LoadingState } from "@/shared/ui/state";
import { LoginForm } from "./login-form";
import { SessionProvider } from "./session-context";

export function AuthBoundary({ children }: { children: React.ReactNode }) {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const session = useQuery({
    queryKey: queryKeys.session,
    queryFn: () => gateway.currentUser(),
    retry: false,
  });
  const [loginError, setLoginError] = React.useState<string | null>(null);

  if (session.isPending) return <LoadingState label="Opening workspace" />;
  if (session.isError && !(session.error instanceof AuthenticationRequiredError)) {
    return <LoginForm error={session.error.message} onSubmit={login} />;
  }
  if (!session.data) return <LoginForm error={loginError} onSubmit={login} />;

  return <SessionProvider user={session.data}>{children}</SessionProvider>;

  async function login(input: { username: string; password: string }) {
    setLoginError(null);
    try {
      const user = await gateway.login(input.username, input.password);
      queryClient.setQueryData(queryKeys.session, user);
    } catch (error) {
      setLoginError(error instanceof Error ? error.message : "Sign in failed");
    }
  }
}

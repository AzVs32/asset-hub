import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { Navigate, Outlet, useLocation } from "react-router";
import { defaultDirectoryPath, LOGIN_PATH } from "@/app/paths";
import { AuthenticationRequiredError } from "@/application/errors";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import type { CurrentUser } from "@/domain/auth";
import { LoadingState } from "@/shared/ui/state";
import { LoginForm } from "./login-form";
import { SessionProvider } from "./session-context";

export function AuthBoundary() {
  const gateway = useGateway();
  const queryClient = useQueryClient();
  const location = useLocation();
  const session = useQuery({
    queryKey: queryKeys.session,
    queryFn: () => gateway.currentUser(),
    retry: false,
  });
  const [loginError, setLoginError] = React.useState<string | null>(null);

  if (session.isPending) return <LoadingState label="Opening workspace" />;
  if (!session.data) {
    if (location.pathname !== LOGIN_PATH) {
      return (
        <Navigate
          to={LOGIN_PATH}
          state={{ from: `${location.pathname}${location.search}${location.hash}` }}
          replace
        />
      );
    }
    const error =
      session.isError && !(session.error instanceof AuthenticationRequiredError)
        ? session.error.message
        : loginError;
    return <LoginForm error={error} onSubmit={login} />;
  }

  if (location.pathname === LOGIN_PATH) {
    return <Navigate to={loginDestination(location.state, session.data)} replace />;
  }

  return (
    <SessionProvider user={session.data}>
      <Outlet />
    </SessionProvider>
  );

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

function loginDestination(state: unknown, user: CurrentUser) {
  if (state && typeof state === "object" && "from" in state) {
    const from = (state as { from?: unknown }).from;
    if (
      typeof from === "string" &&
      from.startsWith("/") &&
      !from.startsWith("//") &&
      from !== LOGIN_PATH
    ) {
      return from;
    }
  }
  return defaultDirectoryPath(user);
}

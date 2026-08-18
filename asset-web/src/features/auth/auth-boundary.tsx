import { Box, CircularProgress } from "@mui/material";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import React from "react";
import { Navigate, Outlet, useLocation } from "react-router";
import { defaultDirectoryPath, LOGIN_PATH } from "@/app/paths";
import { AuthenticationRequiredError } from "@/application/errors";
import { useGateway } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
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

  if (session.isPending) {
    return (
      <Box sx={{ display: "grid", minHeight: "100vh", placeItems: "center" }}>
        <CircularProgress />
      </Box>
    );
  }
  if (!session.data) {
    if (location.pathname !== LOGIN_PATH) {
      return <Navigate to={LOGIN_PATH} replace />;
    }
    const error =
      session.isError && !(session.error instanceof AuthenticationRequiredError)
        ? session.error.message
        : loginError;
    return <LoginForm error={error} onSubmit={login} />;
  }

  if (location.pathname === LOGIN_PATH) {
    return <Navigate to={defaultDirectoryPath()} replace />;
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
      queryClient.removeQueries({
        predicate: (query) => query.queryKey[0] !== queryKeys.session[0],
      });
      queryClient.setQueryData(queryKeys.session, user);
    } catch (error) {
      setLoginError(error instanceof Error ? error.message : "Sign in failed");
    }
  }
}

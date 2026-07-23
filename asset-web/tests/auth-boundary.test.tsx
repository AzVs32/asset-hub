import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import { MemoryRouter, Route, Routes, useLocation } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AuthenticationRequiredError } from "@/application/errors";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import { queryKeys } from "@/application/queries/keys";
import { AuthBoundary } from "@/features/auth/auth-boundary";
import { useSignOut } from "@/features/auth/use-sign-out";

afterEach(cleanup);

describe("AuthBoundary", () => {
  it("shows the login form immediately after signing out", async () => {
    const logout = vi.fn(async () => undefined);
    const currentUser = vi.fn(async () => ({
      id: "admin-1",
      username: "admin",
      role: "administrator" as const,
      isAdmin: true,
    }));
    const gateway = { currentUser, logout } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    queryClient.setQueryData(queryKeys.users, [{ id: "stale-user" }]);
    const user = userEvent.setup();

    render(
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <MemoryRouter initialEntries={["/"]}>
            <TestRoutes protectedElement={<ProtectedContent />} />
          </MemoryRouter>
        </GatewayProvider>
      </QueryClientProvider>,
    );

    await user.click(await screen.findByRole("button", { name: "Sign out" }));

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Asset Hub" })).toBeInTheDocument();
    });
    expect(screen.getByLabelText("Username")).toBeInTheDocument();
    expect(logout).toHaveBeenCalledOnce();
    expect(currentUser).toHaveBeenCalledOnce();
    expect(queryClient.getQueryData(queryKeys.session)).toBeNull();
    expect(queryClient.getQueryData(queryKeys.users)).toBeUndefined();
    expect(screen.getByTestId("location")).toHaveTextContent("/login");
    queryClient.clear();
  });

  it("starts a fresh root workspace after signing in", async () => {
    const currentUser = vi.fn(async () => {
      throw new AuthenticationRequiredError();
    });
    const login = vi.fn(async () => ({
      id: "admin-1",
      username: "admin",
      role: "administrator" as const,
      isAdmin: true,
    }));
    const gateway = { currentUser, login } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    queryClient.setQueryData(queryKeys.resource("file-1"), { id: "file-1" });
    const user = userEvent.setup();

    render(
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <MemoryRouter initialEntries={["/users/azvs/images?resource=file-1&page=2&q=photo"]}>
            <TestRoutes protectedElement={<div>Files</div>} />
          </MemoryRouter>
        </GatewayProvider>
      </QueryClientProvider>,
    );

    await screen.findByLabelText("Username");
    expect(screen.getByTestId("location")).toHaveTextContent("/login");
    await user.type(screen.getByLabelText("Username"), "admin");
    await user.type(screen.getByLabelText("Password"), "password");
    await user.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => {
      expect(screen.getByTestId("location")).toHaveTextContent(/^\/$/);
    });
    expect(screen.getByText("Files")).toBeInTheDocument();
    expect(queryClient.getQueryData(queryKeys.resource("file-1"))).toBeUndefined();
    queryClient.clear();
  });

  it("opens a member's workspace when signing in directly", async () => {
    const currentUser = vi.fn(async () => {
      throw new AuthenticationRequiredError();
    });
    const login = vi.fn(async () => ({
      id: "azvs-1",
      username: "azvs",
      role: "member" as const,
      isAdmin: false,
    }));
    const gateway = { currentUser, login } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const user = userEvent.setup();

    render(
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <MemoryRouter initialEntries={["/login"]}>
            <TestRoutes protectedElement={<div>Files</div>} />
          </MemoryRouter>
        </GatewayProvider>
      </QueryClientProvider>,
    );

    await user.type(await screen.findByLabelText("Username"), "azvs");
    await user.type(screen.getByLabelText("Password"), "password");
    await user.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => {
      expect(screen.getByTestId("location")).toHaveTextContent("/");
    });
    queryClient.clear();
  });
});

function TestRoutes({ protectedElement }: { protectedElement: ReactNode }) {
  return (
    <>
      <LocationProbe />
      <Routes>
        <Route element={<AuthBoundary />}>
          <Route path="/login" element={null} />
          <Route path="/*" element={protectedElement} />
        </Route>
      </Routes>
    </>
  );
}

function LocationProbe() {
  const location = useLocation();
  return <output data-testid="location">{`${location.pathname}${location.search}`}</output>;
}

function ProtectedContent() {
  const signOut = useSignOut();
  return (
    <button type="button" onClick={() => void signOut()}>
      Sign out
    </button>
  );
}

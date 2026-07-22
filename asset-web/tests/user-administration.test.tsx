import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import { UserAdministration } from "@/features/users/user-administration";

describe("UserAdministration", () => {
  it("creates a user without asking for a workspace directory", async () => {
    const createUser = vi.fn(
      async (_input: { username: string; password: string; isAdmin: boolean }) => undefined,
    );
    const gateway = {
      listUsers: async () => [],
      createUser,
      updateUserStatus: async () => {
        throw new Error("not used");
      },
    } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const user = userEvent.setup();

    render(
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <UserAdministration open onOpenChange={() => undefined} currentUserId="admin-1" />
        </GatewayProvider>
      </QueryClientProvider>,
    );

    expect(screen.queryByLabelText("Workspace directory")).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("Username"), "alice");
    await user.type(screen.getByLabelText("Password"), "alice-password");
    await user.click(screen.getByRole("button", { name: "Create user" }));

    await waitFor(() => {
      expect(createUser.mock.calls[0]?.[0]).toEqual({
        username: "alice",
        password: "alice-password",
        isAdmin: false,
      });
    });
    queryClient.clear();
  });
});

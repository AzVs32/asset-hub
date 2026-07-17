import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router";
import { describe, expect, it } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import type { ResourceFilters } from "@/domain/resource";
import { SessionProvider } from "@/features/auth/session-context";
import { useResourceListing } from "@/features/resources/hooks/use-resource-listing";

describe("resource listing navigation", () => {
  it("enters a directory and clears stale page and resource state atomically", async () => {
    const gateway = {
      listResourceKinds: async () => [],
      listDirectory: async (filters: ResourceFilters) => ({
        path: filters.directory,
        folders: [],
        resources: { items: [], total: 0, page: filters.page, limit: filters.limit },
      }),
      listDirectoryGrants: async () => [],
    } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <SessionProvider
            user={{
              id: "admin-1",
              username: "admin",
              role: "administrator",
              workspaceDirectory: "",
              isAdmin: true,
            }}
          >
            <MemoryRouter
              initialEntries={["/?directory=library&page=4&resource=resource-1&q=photo"]}
            >
              {children}
            </MemoryRouter>
          </SessionProvider>
        </GatewayProvider>
      </QueryClientProvider>
    );
    const { result } = renderHook(() => useResourceListing(), { wrapper });

    act(() => result.current.openDirectory("library/images"));

    await waitFor(() => {
      expect(result.current.filters).toMatchObject({
        directory: "library/images",
        page: 1,
        query: "photo",
      });
      expect(result.current.selectedId).toBeNull();
    });

    queryClient.clear();
  });
});

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router";
import { describe, expect, it } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import { WorkspaceScopeProvider } from "@/application/workspace/workspace-scope-context";
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
            <WorkspaceScopeProvider
              user={{
                id: "admin-1",
                username: "admin",
                role: "administrator",
                workspaceDirectory: "",
                isAdmin: true,
              }}
            >
              <MemoryRouter initialEntries={["/library?page=4&resource=resource-1&q=photo"]}>
                <Routes>
                  <Route path="/*" element={children} />
                </Routes>
              </MemoryRouter>
            </WorkspaceScopeProvider>
          </SessionProvider>
        </GatewayProvider>
      </QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({ browser: useResourceListing(), location: useLocation(), navigate: useNavigate() }),
      { wrapper },
    );

    act(() => result.current.browser.openDirectory("library/images"));

    await waitFor(() => {
      expect(result.current.browser.filters).toMatchObject({
        directory: "library/images",
        page: 1,
        query: "photo",
      });
      expect(result.current.browser.selectedId).toBeNull();
      expect(result.current.location.pathname).toBe("/library/images");
      expect(result.current.location.search).toBe("?q=photo");
    });

    act(() => result.current.navigate(-1));
    await waitFor(() => {
      expect(result.current.location.pathname).toBe("/library");
    });

    queryClient.clear();
  });

  it("treats a member workspace as the visible root", async () => {
    const requestedDirectories: string[] = [];
    const gateway = {
      listResourceKinds: async () => [],
      listDirectory: async (filters: ResourceFilters) => {
        requestedDirectories.push(filters.directory);
        return {
          path: filters.directory,
          folders: [],
          resources: { items: [], total: 0, page: filters.page, limit: filters.limit },
        };
      },
    } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });

    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <SessionProvider
            user={{
              id: "azvs-1",
              username: "azvs",
              role: "member",
              workspaceDirectory: "users/azvs",
              isAdmin: false,
            }}
          >
            <WorkspaceScopeProvider
              user={{
                id: "azvs-1",
                username: "azvs",
                role: "member",
                workspaceDirectory: "users/azvs",
                isAdmin: false,
              }}
            >
              <MemoryRouter initialEntries={["/images?q=photo"]}>
                <Routes>
                  <Route path="/*" element={children} />
                </Routes>
              </MemoryRouter>
            </WorkspaceScopeProvider>
          </SessionProvider>
        </GatewayProvider>
      </QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({ browser: useResourceListing(), location: useLocation() }),
      { wrapper },
    );

    await waitFor(() => {
      expect(requestedDirectories).toContain("users/azvs/images");
    });
    expect(result.current.browser.filters.directory).toBe("images");
    expect(result.current.location.pathname).toBe("/images");

    act(() => result.current.browser.openDirectory("images/raw"));
    await waitFor(() => {
      expect(result.current.location.pathname).toBe("/images/raw");
      expect(result.current.browser.filters.directory).toBe("images/raw");
    });

    act(() => result.current.browser.openDirectory(""));
    await waitFor(() => {
      expect(result.current.location.pathname).toBe("/");
      expect(result.current.browser.filters.directory).toBe("");
    });

    queryClient.clear();
  });

  it("canonicalizes a member's real workspace URL to a visible relative path", async () => {
    const gateway = {
      listResourceKinds: async () => [],
      listDirectory: async (filters: ResourceFilters) => ({
        path: filters.directory,
        folders: [],
        resources: { items: [], total: 0, page: filters.page, limit: filters.limit },
      }),
    } as unknown as AssetGateway;
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>
        <GatewayProvider gateway={gateway}>
          <SessionProvider
            user={{
              id: "azvs-1",
              username: "azvs",
              role: "member",
              workspaceDirectory: "users/azvs",
              isAdmin: false,
            }}
          >
            <WorkspaceScopeProvider
              user={{
                id: "azvs-1",
                username: "azvs",
                role: "member",
                workspaceDirectory: "users/azvs",
                isAdmin: false,
              }}
            >
              <MemoryRouter initialEntries={["/users/azvs/images?q=photo"]}>
                <Routes>
                  <Route path="/*" element={children} />
                </Routes>
              </MemoryRouter>
            </WorkspaceScopeProvider>
          </SessionProvider>
        </GatewayProvider>
      </QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({ browser: useResourceListing(), location: useLocation() }),
      { wrapper },
    );

    await waitFor(() => {
      expect(result.current.location.pathname).toBe("/images");
    });
    expect(result.current.location.search).toBe("?q=photo");
    expect(result.current.browser.filters.directory).toBe("images");
    queryClient.clear();
  });
});

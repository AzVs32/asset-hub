import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter, Route, Routes, useLocation, useNavigate } from "react-router";
import { describe, expect, it } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import type { ResourceFilters } from "@/domain/resource";
import { useResourceListing } from "@/features/resources/hooks/use-resource-listing";

describe("resource listing navigation", () => {
  it("uses URL paths as backend-relative directories", async () => {
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
          <MemoryRouter initialEntries={["/images?page=4&resource=resource-1&q=photo"]}>
            <Routes>
              <Route path="/*" element={children} />
            </Routes>
          </MemoryRouter>
        </GatewayProvider>
      </QueryClientProvider>
    );
    const { result } = renderHook(
      () => ({ browser: useResourceListing(), location: useLocation(), navigate: useNavigate() }),
      { wrapper },
    );

    await waitFor(() => expect(requestedDirectories).toContain("images"));
    expect(result.current.browser.filters.directory).toBe("images");

    act(() => result.current.browser.openDirectory("images/raw"));
    await waitFor(() => {
      expect(result.current.location.pathname).toBe("/images/raw");
      expect(result.current.location.search).toBe("?q=photo");
      expect(result.current.browser.filters.directory).toBe("images/raw");
      expect(result.current.browser.selectedId).toBeNull();
    });

    act(() => result.current.browser.openDirectory(""));
    await waitFor(() => {
      expect(result.current.location.pathname).toBe("/");
      expect(result.current.browser.filters.directory).toBe("");
    });

    act(() => result.current.navigate(-1));
    await waitFor(() => expect(result.current.location.pathname).toBe("/images/raw"));
    queryClient.clear();
  });
});

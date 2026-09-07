import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MemoryRouter, useLocation } from "react-router";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DirectoryListing } from "@/domain/directory";
import type { Resource } from "@/domain/resource";
import { ConcurrentModificationError } from "@/shared/api/errors";
import { queryKeys } from "@/shared/api/query-keys";
import { useAssetWorkspaceCommands } from "./hooks/use-asset-workspace-commands";
import { useAssetWorkspaceListing } from "./hooks/use-asset-workspace-listing";

const gateway = vi.hoisted(() => ({
  listDirectory: vi.fn(),
  findResource: vi.fn(),
  updateResource: vi.fn(),
}));
vi.mock("@/shared/api/gateway-context", () => ({ useAssetWorkspaceGateway: () => gateway }));
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

const resource: Resource = {
  id: "resource",
  name: "asset",
  directory: "a%20b",
  directoryId: "dir",
  revision: 1,
  state: { lifecycle: { status: "active" }, content: "absent", effective: "no_content" },
  content: null,
  createdAt: "2026-09-07",
  updatedAt: "2026-09-07",
};
const listing: DirectoryListing = {
  path: resource.directory,
  directory: {
    id: "dir",
    path: resource.directory,
    parentId: null,
    parentPath: "",
    name: resource.directory,
    revision: 1,
    createdAt: "",
    updatedAt: "",
  },
  folders: [],
  resources: { items: [resource], page: 1, limit: 30, total: 1 },
};
let client: QueryClient;
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.clearAllMocks();
  client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: Infinity }, mutations: { retry: false } },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  client.clear();
  host.remove();
  vi.unstubAllGlobals();
});

it("decodes raw paths once, normalizes exclusive selection, and never shows the old directory during navigation", async () => {
  gateway.listDirectory.mockImplementation(() => new Promise(() => {}));
  client.setQueryData(
    queryKeys.directory({ directory: resource.directory, page: 1, limit: 30 }),
    listing,
  );
  let state!: ReturnType<typeof useAssetWorkspaceListing>;
  let search = "";
  function Probe() {
    state = useAssetWorkspaceListing();
    search = useLocation().search;
    return null;
  }
  await act(() =>
    root.render(
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={["/a%2520b?resource=resource&folder=dir"]}>
          <Probe />
        </MemoryRouter>
      </QueryClientProvider>,
    ),
  );
  expect(state.filters.directory).toBe("a%20b");
  expect(state.selectedId).toBe("resource");
  expect(state.selectedDirectoryId).toBeNull();
  expect(search).not.toContain("folder=");
  expect(state.listing.data).toEqual(listing);
  await act(() => state.openDirectory("other%2Ffolder"));
  expect(state.filters.directory).toBe("other%2Ffolder");
  expect(state.listing.data).toBeUndefined();
  expect(state.selectedId).toBeNull();
});

it("returns to the last valid page after its last resource is deleted", async () => {
  const filters = { directory: resource.directory, page: 2, limit: 30 };
  client.setQueryData(queryKeys.directory(filters), {
    ...listing,
    resources: { items: [], total: 30, page: 2, limit: 30 },
  });
  client.setQueryData(queryKeys.directory({ ...filters, page: 1 }), listing);
  let state!: ReturnType<typeof useAssetWorkspaceListing>;
  function Probe() {
    state = useAssetWorkspaceListing();
    return null;
  }
  await act(() =>
    root.render(
      <QueryClientProvider client={client}>
        <MemoryRouter initialEntries={["/a%2520b?page=2&resource=resource"]}>
          <Probe />
        </MemoryRouter>
      </QueryClientProvider>,
    ),
  );
  expect(state.filters.page).toBe(1);
  expect(state.selectedId).toBeNull();
});

it("replaces the detail snapshot after a revision conflict and invalidates only the source and new destination", async () => {
  const latest = { ...resource, revision: 2, directory: "moved" };
  gateway.updateResource.mockRejectedValue(new ConcurrentModificationError());
  gateway.findResource.mockResolvedValue(latest);
  client.setQueryData(queryKeys.resource(resource.id), resource);
  for (const directory of [resource.directory, "moved", "unrelated"]) {
    client.setQueryData(queryKeys.directory({ directory, page: 1, limit: 30 }), listing);
  }
  let commands!: ReturnType<typeof useAssetWorkspaceCommands>;
  function Probe() {
    commands = useAssetWorkspaceCommands();
    return null;
  }
  await act(() =>
    root.render(
      <QueryClientProvider client={client}>
        <Probe />
      </QueryClientProvider>,
    ),
  );
  await act(async () => {
    await expect(
      commands.update.mutateAsync({
        resource,
        draft: { name: "edited", directory: resource.directory },
      }),
    ).rejects.toBeInstanceOf(ConcurrentModificationError);
  });
  expect(client.getQueryData(queryKeys.resource(resource.id))).toEqual(latest);
  for (const directory of [resource.directory, "moved", "unrelated"]) {
    expect(
      client.getQueryState(queryKeys.directory({ directory, page: 1, limit: 30 }))?.isInvalidated,
    ).toBe(directory !== "unrelated");
  }
});

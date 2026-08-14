import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import { DirectoryWorkspaceOutlet } from "@/features/resources/directory-workspace-outlet";
import { PluginKernel, PluginKernelProvider } from "@/kernel/plugin-kernel";
import { directoryWorkspaceOutlet } from "@/kernel/slots";
import { directory, directoryAction } from "./fixtures";

vi.mock("@/plugins/directory-plugin-frame", () => ({
  DirectoryPluginFrame: ({ view }: { view: { title?: string } }) => (
    <iframe title={view.title ?? "Directory plugin workspace"} />
  ),
}));

afterEach(cleanup);

describe("DirectoryWorkspaceOutlet", () => {
  it("mounts CoreDirectoryWorkspace only when no plugin workspace provider applies", () => {
    renderOutlet({
      directory: directory(),
      gateway: {} as AssetGateway,
    });

    expect(screen.getByText("Core workspace")).toBeInTheDocument();
  });

  it("hands the complete content workspace to the plugin frame", async () => {
    const workspace = directoryAction({
      id: "example.game.workspace",
      provides: "workspace",
      output: { views: ["plugin_frame", "json"], effects: [] },
      ui: { locations: [directoryWorkspaceOutlet] },
    });
    const item = directory([workspace]);
    const executeDirectoryAction = vi.fn().mockResolvedValue({
      directoryId: item.id,
      action: workspace.id,
      diagnostics: [],
      effects: [],
      view: {
        view: "plugin_frame",
        plugin_api: "asset-hub.plugin-api@4",
        title: "Game library",
        url: "/plugins/example.game/index.html",
      },
    });
    renderOutlet({
      directory: item,
      gateway: {
        executeDirectoryAction,
        assetUrl: (path: string) => path,
      } as unknown as AssetGateway,
    });

    expect(await screen.findByTitle("Game library")).toBeInTheDocument();
    expect(screen.queryByText("Core workspace")).not.toBeInTheDocument();
    expect(executeDirectoryAction).toHaveBeenCalledWith(item, workspace);
  });
});

function renderOutlet({
  directory: current,
  gateway,
}: {
  directory: ReturnType<typeof directory>;
  gateway: AssetGateway;
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Number.POSITIVE_INFINITY } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <GatewayProvider gateway={gateway}>
        <PluginKernelProvider kernel={new PluginKernel()}>
          <DirectoryWorkspaceOutlet
            directory={current}
            coreWorkspace={<div>Core workspace</div>}
            onDirectoryChanged={vi.fn()}
            onNavigate={vi.fn()}
          />
        </PluginKernelProvider>
      </GatewayProvider>
    </QueryClientProvider>,
  );
}

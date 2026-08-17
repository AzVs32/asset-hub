import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { DirectoryActionOutput, PluginView } from "@/domain/plugin";
import { DirectoryPluginFrame } from "@/plugins/directory-plugin-frame";
import { directory } from "./fixtures";

const { destroy } = vi.hoisted(() => ({ destroy: vi.fn() }));

vi.mock("penpal", () => ({
  connect: vi.fn(() => ({ destroy })),
  WindowMessenger: class {},
}));

const mounted: Array<ReturnType<typeof createRoot>> = [];

afterEach(async () => {
  await act(async () => {
    for (const root of mounted.splice(0)) root.unmount();
  });
  destroy.mockClear();
});

describe("DirectoryPluginFrame", () => {
  it("replaces the iframe when navigation changes the bound Directory", async () => {
    const container = document.createElement("div");
    const root = createRoot(container);
    mounted.push(root);
    const firstDirectory = directory([]);
    const secondDirectory = {
      ...firstDirectory,
      id: "directory-2",
      path: "collections/item",
      name: "Item",
    };
    const view: Extract<PluginView, { view: "plugin_frame" }> = {
      view: "plugin_frame",
      plugin_api: "asset-hub.plugin-api@1",
      title: "Collection workspace",
      url: "/plugins/example.collection/index.html",
    };
    const gateway = {
      assetUrl: (url: string) => url,
    } as unknown as AssetGateway;

    await act(async () => {
      root.render(
        <DirectoryPluginFrame
          directory={firstDirectory}
          output={output(firstDirectory.id, view)}
          view={view}
          gateway={gateway}
        />,
      );
    });
    const firstFrame = container.querySelector("iframe");

    await act(async () => {
      root.render(
        <DirectoryPluginFrame
          directory={secondDirectory}
          output={output(secondDirectory.id, view)}
          view={view}
          gateway={gateway}
        />,
      );
    });

    expect(container.querySelector("iframe")).not.toBe(firstFrame);
  });
});

function output(
  directoryId: string,
  view: Extract<PluginView, { view: "plugin_frame" }>,
): DirectoryActionOutput {
  return {
    directoryId,
    action: "example.collection.workspace",
    diagnostics: [],
    effects: [],
    view,
  };
}

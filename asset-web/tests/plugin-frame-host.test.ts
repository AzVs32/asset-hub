import { describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { ResourceActionOutput } from "@/domain/plugin";
import { createDirectoryPluginFrameHostBridge } from "@/plugins/directory-frame-host";
import { createPluginFrameHostBridge } from "@/plugins/frame-host";
import { action, directory, directoryAction, resource } from "./fixtures";

describe("Plugin Frame host bridge", () => {
  it("replaces text only for the frame's write text_edit provider and advances its revision", async () => {
    const edit = action({
      id: "example.content.edit",
      provides: "text_edit",
      access: "write",
    });
    const initial = resource([edit]);
    const revised = { ...initial, revision: 2 };
    const replaceResourceText = vi
      .fn()
      .mockResolvedValueOnce(revised)
      .mockResolvedValueOnce({ ...revised, revision: 3 });
    const onResourceChanged = vi.fn().mockResolvedValue(undefined);
    const bridge = createPluginFrameHostBridge({
      resource: initial,
      frameResourceId: "resource-1",
      frameActionId: edit.id,
      gateway: { replaceResourceText } as unknown as AssetGateway,
      onResourceChanged,
    });

    await bridge.methods.replaceResourceText("# First");
    await bridge.methods.replaceResourceText("# Second");

    expect(replaceResourceText).toHaveBeenNthCalledWith(1, initial, "# First");
    expect(replaceResourceText).toHaveBeenNthCalledWith(2, revised, "# Second");
    expect(onResourceChanged).toHaveBeenCalledTimes(2);
  });

  it("denies replacement when the frame was opened by a read action", async () => {
    const read = action({ id: "example.content.read" });
    const edit = action({
      id: "example.content.edit",
      provides: "text_edit",
      access: "write",
    });
    const replaceResourceText = vi.fn();
    const bridge = createPluginFrameHostBridge({
      resource: resource([read, edit]),
      frameResourceId: "resource-1",
      frameActionId: read.id,
      gateway: { replaceResourceText } as unknown as AssetGateway,
    });

    await expect(bridge.methods.replaceResourceText("# Not allowed")).rejects.toThrow(
      "Text editing is not available from this frame.",
    );
    expect(replaceResourceText).not.toHaveBeenCalled();
  });

  it("executes only actions exposed by the bound Resource and rejects malformed input", async () => {
    const inspect = action({ id: "example.inspect" });
    const item = resource([inspect]);
    const expected = pluginOutput(inspect.id);
    const executeResourceAction = vi.fn().mockResolvedValue(expected);
    const bridge = createPluginFrameHostBridge({
      resource: item,
      frameResourceId: "resource-1",
      frameActionId: inspect.id,
      gateway: { executeResourceAction } as unknown as AssetGateway,
    });

    await expect(
      bridge.methods.executeResourceAction(inspect.id, { operation: "load" }),
    ).resolves.toBe(expected);
    expect(executeResourceAction).toHaveBeenCalledWith(item, inspect.id, { operation: "load" });
    await expect(bridge.methods.executeResourceAction("missing", {})).rejects.toThrow(
      "Action missing is not available.",
    );
    await expect(bridge.methods.executeResourceAction(inspect.id, [])).rejects.toThrow(
      "Action input must be a JSON object.",
    );
  });

  it("requires host confirmation before a destructive action from a plugin frame", async () => {
    const remove = action({
      id: "core.resource.delete",
      access: "write",
      output: { views: [], effects: ["delete"] },
      ui: { destructive: true, confirmation: "Delete Example?" },
    });
    const expected: ResourceActionOutput = {
      resourceId: "resource-1",
      action: remove.id,
      diagnostics: [],
      view: null,
      effects: ["delete"],
    };
    const executeResourceAction = vi.fn().mockResolvedValue(expected);
    const confirmAction = vi.fn().mockResolvedValue(false);
    const bridge = createPluginFrameHostBridge({
      resource: resource([remove]),
      frameResourceId: "resource-1",
      frameActionId: "example.frame",
      gateway: { executeResourceAction } as unknown as AssetGateway,
      confirmAction,
    });

    await expect(bridge.methods.executeResourceAction(remove.id, {})).rejects.toThrow(
      `Action ${remove.id} was not confirmed.`,
    );
    expect(executeResourceAction).not.toHaveBeenCalled();

    confirmAction.mockResolvedValueOnce(true);
    await expect(bridge.methods.executeResourceAction(remove.id, {})).resolves.toBe(expected);
    expect(confirmAction).toHaveBeenCalledWith("Delete Example?");
    expect(executeResourceAction).toHaveBeenCalledWith(resource([remove]), remove.id, {});
  });
});

describe("Directory Plugin Frame host bridge", () => {
  it("executes only actions exposed by the bound Directory", async () => {
    const inspect = directoryAction({ id: "example.game.load" });
    const item = directory([inspect]);
    const expected = {
      directoryId: item.id,
      action: inspect.id,
      diagnostics: [],
      view: { view: "json" as const, data: { games: [] } },
      effects: [],
    };
    const executeDirectoryAction = vi.fn().mockResolvedValue(expected);
    const bridge = createDirectoryPluginFrameHostBridge({
      directory: item,
      frameDirectoryId: item.id,
      gateway: { executeDirectoryAction } as unknown as AssetGateway,
    });

    await expect(
      bridge.methods.executeDirectoryAction(inspect.id, { operation: "load" }),
    ).resolves.toBe(expected);
    expect(executeDirectoryAction).toHaveBeenCalledWith(item, inspect.id, { operation: "load" });
    await expect(bridge.methods.executeDirectoryAction("missing", {})).rejects.toThrow(
      "Action missing is not available.",
    );
  });

  it("keeps navigation, refresh, and destructive confirmation inside the Host boundary", async () => {
    const remove = directoryAction({
      id: "core.directory.delete",
      access: "write",
      output: { views: [], effects: ["delete"] },
      ui: { destructive: true, confirmation: "Delete {name}?" },
    });
    const item = directory([remove]);
    const executeDirectoryAction = vi.fn().mockResolvedValue({
      directoryId: item.id,
      action: remove.id,
      diagnostics: [],
      view: null,
      effects: ["delete"],
    });
    const onDirectoryChanged = vi.fn().mockResolvedValue(undefined);
    const onNavigate = vi.fn().mockResolvedValue(undefined);
    const confirmAction = vi.fn().mockResolvedValue(true);
    const bridge = createDirectoryPluginFrameHostBridge({
      directory: item,
      frameDirectoryId: item.id,
      gateway: { executeDirectoryAction } as unknown as AssetGateway,
      onDirectoryChanged,
      onNavigate,
      confirmAction,
    });

    await bridge.methods.executeDirectoryAction(remove.id, {});
    await bridge.methods.refreshDirectory();
    await bridge.methods.navigateToDirectory("library/games");

    expect(confirmAction).toHaveBeenCalledWith("Delete Library?");
    expect(onDirectoryChanged).toHaveBeenCalledTimes(2);
    expect(onNavigate).toHaveBeenCalledWith("library/games");
    await expect(bridge.methods.navigateToDirectory("../outside")).rejects.toThrow(
      "Directory path must be a canonical relative path.",
    );
  });
});

describe("Plugin Frame aggregate binding", () => {
  it("never rebinds an existing Resource or Directory frame", () => {
    const currentResource = resource([]);
    const resourceBridge = createPluginFrameHostBridge({
      resource: currentResource,
      frameResourceId: currentResource.id,
      frameActionId: "example.frame",
      gateway: {} as AssetGateway,
    });
    expect(() =>
      resourceBridge.updateResource({ ...currentResource, id: "another-resource" }),
    ).toThrow("cannot change its bound Resource");

    const currentDirectory = directory([]);
    const directoryBridge = createDirectoryPluginFrameHostBridge({
      directory: currentDirectory,
      frameDirectoryId: currentDirectory.id,
      gateway: {} as AssetGateway,
    });
    expect(() =>
      directoryBridge.updateDirectory({ ...currentDirectory, id: "another-directory" }),
    ).toThrow("cannot change its bound Directory");
  });
});

function pluginOutput(actionId: string): ResourceActionOutput {
  return {
    resourceId: "resource-1",
    action: actionId,
    diagnostics: [],
    effects: [],
    view: {
      view: "plugin_frame",
      plugin_api: "asset-hub.plugin-api@5",
      title: "Plugin frame",
      url: "/plugins/example/index.html",
    },
  };
}

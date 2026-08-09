import { describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { ResourceActionOutput } from "@/domain/plugin";
import { createPluginFrameHostBridge } from "@/plugins/frame-host";
import { action, resource } from "./fixtures";

describe("Plugin Frame host bridge", () => {
  it("replaces text only for the frame's write text_edit provider and advances its revision", async () => {
    const edit = action({
      id: "azvs.markdown.edit",
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
    const read = action({ id: "azvs.markdown.read", provides: "text_read" });
    const edit = action({
      id: "azvs.markdown.edit",
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
    const executeAction = vi.fn().mockResolvedValue(expected);
    const bridge = createPluginFrameHostBridge({
      resource: item,
      frameResourceId: "resource-1",
      frameActionId: inspect.id,
      gateway: { executeAction } as unknown as AssetGateway,
    });

    await expect(
      bridge.methods.executeResourceAction(inspect.id, { operation: "load" }),
    ).resolves.toBe(expected);
    expect(executeAction).toHaveBeenCalledWith(item, inspect.id, { operation: "load" });
    await expect(bridge.methods.executeResourceAction("missing", {})).rejects.toThrow(
      "Action missing is not available.",
    );
    await expect(bridge.methods.executeResourceAction(inspect.id, [])).rejects.toThrow(
      "Action input must be a JSON object.",
    );
  });
});

function pluginOutput(actionId: string): ResourceActionOutput {
  return {
    resourceId: "resource-1",
    action: actionId,
    diagnostics: [],
    view: {
      view: "plugin_frame",
      plugin_api: "asset-hub.plugin-api@3",
      title: "Plugin frame",
      url: "/plugins/example/index.html",
    },
  };
}

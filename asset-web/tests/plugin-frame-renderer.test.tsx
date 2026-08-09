import { render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import type { ResourceActionOutput } from "@/domain/plugin";
import { PluginKernel, PluginKernelProvider } from "@/kernel/plugin-kernel";
import { PluginViewHost } from "@/kernel/plugin-view-host";
import { registerDefaultViewRenderers } from "@/plugins/renderers/default-renderers";
import { action, resource } from "./fixtures";

const pluginApi = "asset-hub.plugin-api@2";

describe("PluginFrameView", () => {
  it("replaces text through the host and advances the frame revision", async () => {
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
    const { frame, postMessage } = renderFrame(
      initial,
      pluginOutput(edit.id),
      { replaceResourceText },
      onResourceChanged,
    );

    sendReplaceRequest(frame, "request-1", "# First");
    await waitFor(() => expect(replaceResourceText).toHaveBeenCalledWith(initial, "# First"));
    await waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "asset-hub:replace-resource-text-result",
          request_id: "request-1",
          ok: true,
        }),
        "*",
      ),
    );

    sendReplaceRequest(frame, "request-2", "# Second");
    await waitFor(() => expect(replaceResourceText).toHaveBeenLastCalledWith(revised, "# Second"));
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
    const { frame, postMessage } = renderFrame(resource([read, edit]), pluginOutput(read.id), {
      replaceResourceText,
    });

    sendReplaceRequest(frame, "request-denied", "# Not allowed");

    await waitFor(() =>
      expect(postMessage).toHaveBeenCalledWith(
        expect.objectContaining({
          type: "asset-hub:replace-resource-text-result",
          request_id: "request-denied",
          ok: false,
          error: "Text editing is not available from this frame.",
        }),
        "*",
      ),
    );
    expect(replaceResourceText).not.toHaveBeenCalled();
  });
});

function renderFrame(
  item: ReturnType<typeof resource>,
  output: ResourceActionOutput,
  gatewayMethods: Partial<AssetGateway>,
  onResourceChanged = vi.fn(),
) {
  const kernel = new PluginKernel();
  registerDefaultViewRenderers(kernel);
  const gateway = {
    assetUrl: (path: string) => `/api${path}`,
    ...gatewayMethods,
  } as AssetGateway;
  const { container } = render(
    <PluginKernelProvider kernel={kernel}>
      <PluginViewHost
        output={output}
        resource={item}
        gateway={gateway}
        onResourceChanged={onResourceChanged}
      />
    </PluginKernelProvider>,
  );
  const frame = container.querySelector("iframe");
  if (!frame) throw new Error("plugin frame was not rendered");
  if (!frame.contentWindow) throw new Error("iframe has no content window");
  const postMessage = vi.spyOn(frame.contentWindow, "postMessage");
  return { frame, postMessage };
}

function pluginOutput(actionId: string): ResourceActionOutput {
  return {
    resourceId: "resource-1",
    action: actionId,
    diagnostics: [],
    view: {
      view: "plugin_frame",
      plugin_api: pluginApi,
      title: "Markdown editor",
      url: "/plugins/azvs.markdown/index.html",
    },
  };
}

function sendReplaceRequest(frame: HTMLIFrameElement, requestId: string, text: string) {
  window.dispatchEvent(
    new MessageEvent("message", {
      source: frame.contentWindow,
      data: {
        type: "asset-hub:replace-resource-text",
        plugin_api: pluginApi,
        request_id: requestId,
        resource_id: "resource-1",
        text,
      },
    }),
  );
}

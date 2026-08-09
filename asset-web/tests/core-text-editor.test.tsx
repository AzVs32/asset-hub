import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GatewayProvider } from "@/application/ports/gateway-context";
import { PluginKernel, PluginKernelProvider } from "@/kernel/plugin-kernel";
import { CoreTextEditor } from "@/plugins/core-text-editor";
import { PluginActionDialog } from "@/plugins/plugin-action-dialog";
import { registerDefaultViewRenderers } from "@/plugins/renderers/default-renderers";
import { action, resource } from "./fixtures";

describe("CoreTextEditor", () => {
  it("saves pure text through the selected core edit action", async () => {
    const user = userEvent.setup();
    const replaceResourceText = vi.fn().mockResolvedValue({});
    const onSaved = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    const textAction = action({
      id: "core.text.edit",
      access: "write",
      provides: "text_edit",
    });

    render(
      <GatewayProvider gateway={{ replaceResourceText } as unknown as AssetGateway}>
        <CoreTextEditor
          resource={resource([textAction])}
          initialText="First draft"
          onSaved={onSaved}
          onClose={onClose}
        />
      </GatewayProvider>,
    );

    const editor = screen.getByRole("textbox", { name: "Text content" });
    await user.clear(editor);
    await user.type(editor, "Updated text");
    await user.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(replaceResourceText).toHaveBeenCalledWith(
        expect.objectContaining({ id: "resource-1" }),
        "Updated text",
      );
    });
    expect(onSaved).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("does not impose the core editor protocol on plugin-owned text edit providers", () => {
    const pluginAction = action({
      id: "example.text.edit",
      access: "write",
      provides: "text_edit",
    });
    const item = resource([pluginAction]);
    const kernel = new PluginKernel();
    registerDefaultViewRenderers(kernel);

    render(
      <GatewayProvider gateway={{} as AssetGateway}>
        <PluginKernelProvider kernel={kernel}>
          <PluginActionDialog
            result={{
              resource: item,
              action: pluginAction,
              output: {
                resourceId: item.id,
                action: pluginAction.id,
                diagnostics: [],
                view: { view: "text", text: "Provider-owned output" },
              },
            }}
            onClose={vi.fn()}
            onResourceChanged={vi.fn()}
          />
        </PluginKernelProvider>
      </GatewayProvider>,
    );

    expect(screen.getByText("Provider-owned output")).toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Text content" })).not.toBeInTheDocument();
  });
});

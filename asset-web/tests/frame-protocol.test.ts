import { describe, expect, it } from "vitest";
import { parseExecuteActionMessage, parsePluginFrameRequest } from "@/plugins/frame-protocol";

describe("plugin frame messages", () => {
  const pluginApi = "asset-hub.plugin-api@2";

  it("accepts a bounded action request", () => {
    expect(
      parseExecuteActionMessage(
        {
          type: "asset-hub:execute-resource-action",
          plugin_api: pluginApi,
          request_id: "request-1",
          resource_id: "resource-1",
          action: "plugin:edit",
          input: { title: "Updated" },
        },
        pluginApi,
      ),
    ).toMatchObject({ requestId: "request-1", resourceId: "resource-1", action: "plugin:edit" });
  });

  it("rejects missing or unknown plugin APIs and malformed input", () => {
    expect(
      parseExecuteActionMessage(
        {
          type: "asset-hub:execute-resource-action",
          request_id: "request-1",
          resource_id: "resource-1",
          action: "plugin:load",
        },
        pluginApi,
      ),
    ).toBeNull();
    expect(
      parseExecuteActionMessage(
        {
          type: "asset-hub:execute-resource-action",
          plugin_api: "asset-hub.plugin-api@2",
        },
        pluginApi,
      ),
    ).toBeNull();
    expect(
      parseExecuteActionMessage(
        {
          type: "asset-hub:execute-resource-action",
          plugin_api: pluginApi,
          request_id: "request-1",
          resource_id: "resource-1",
          action: "x".repeat(129),
        },
        pluginApi,
      ),
    ).toBeNull();
    expect(
      parseExecuteActionMessage(
        {
          type: "asset-hub:execute-resource-action",
          plugin_api: pluginApi,
          request_id: "request-1",
          resource_id: "resource-1",
          action: "plugin:load",
          input: ["not", "an", "object"],
        },
        pluginApi,
      ),
    ).toBeNull();
  });

  it("accepts text replacement only with valid common fields and string content", () => {
    expect(
      parsePluginFrameRequest(
        {
          type: "asset-hub:replace-resource-text",
          plugin_api: pluginApi,
          request_id: "request-2",
          resource_id: "resource-1",
          text: "# Updated",
        },
        pluginApi,
      ),
    ).toEqual({
      type: "asset-hub:replace-resource-text",
      pluginApi,
      requestId: "request-2",
      resourceId: "resource-1",
      text: "# Updated",
    });

    expect(
      parsePluginFrameRequest(
        {
          type: "asset-hub:replace-resource-text",
          plugin_api: pluginApi,
          request_id: "request-2",
          resource_id: "resource-1",
          text: { markdown: "not a string" },
        },
        pluginApi,
      ),
    ).toBeNull();
    expect(
      parsePluginFrameRequest(
        {
          type: "asset-hub:replace-resource-text",
          plugin_api: pluginApi,
          request_id: "request-2",
          resource_id: "another-resource",
          text: "# Updated",
        },
        "another.plugin-api@1",
      ),
    ).toBeNull();
  });
});

import { describe, expect, it } from "vitest";
import { parseExecuteActionMessage } from "@/plugins/frame-protocol";

describe("plugin frame protocol", () => {
  it("accepts a bounded action request", () => {
    expect(
      parseExecuteActionMessage({
        type: "asset-hub:execute-resource-action",
        version: 1,
        request_id: "request-1",
        resource_id: "resource-1",
        action: "plugin:edit",
        input: { title: "Updated" },
      }),
    ).toMatchObject({ requestId: "request-1", resourceId: "resource-1", action: "plugin:edit" });
  });

  it("treats an omitted version as the current v1 protocol used by bundled plugins", () => {
    expect(
      parseExecuteActionMessage({
        type: "asset-hub:execute-resource-action",
        request_id: "request-1",
        resource_id: "resource-1",
        action: "plugin:load",
      })?.version,
    ).toBe(1);
  });

  it("rejects unknown versions and malformed input", () => {
    expect(
      parseExecuteActionMessage({ type: "asset-hub:execute-resource-action", version: 2 }),
    ).toBeNull();
    expect(
      parseExecuteActionMessage({
        type: "asset-hub:execute-resource-action",
        version: 1,
        request_id: "request-1",
        resource_id: "resource-1",
        action: "x".repeat(129),
      }),
    ).toBeNull();
    expect(
      parseExecuteActionMessage({
        type: "asset-hub:execute-resource-action",
        version: 1,
        request_id: "request-1",
        resource_id: "resource-1",
        action: "plugin:load",
        input: ["not", "an", "object"],
      }),
    ).toBeNull();
  });
});

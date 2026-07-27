import { describe, expect, it } from "vitest";
import { parsePluginView } from "@/infrastructure/http/plugin-view-schema";

describe("plugin view boundary", () => {
  it("accepts download views and rejects the removed binary URL view", () => {
    expect(
      parsePluginView({
        view: "download",
        url: "/resources/resource-1/download",
        filename: "asset.bin",
      }).view,
    ).toBe("download");
    expect(() =>
      parsePluginView({
        view: "binary_url",
        url: "/resources/resource-1/content",
      }),
    ).toThrow();
  });

  it("rejects untrusted output that does not match a host renderer contract", () => {
    expect(() => parsePluginView({ view: "plugin_frame", url: 42 })).toThrow();
    expect(() => parsePluginView({ view: "script", code: "alert(1)" })).toThrow();
    expect(() => parsePluginView({ view: "table", columns: [], rows: [] })).toThrow();
    expect(() => parsePluginView({ view: "form", schema: {} })).toThrow();
  });
});

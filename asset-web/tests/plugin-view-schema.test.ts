import { describe, expect, it } from "vitest";
import { parsePluginView } from "@/infrastructure/http/plugin-view-schema";

describe("plugin view boundary", () => {
  it("preserves the Plugin API carried by frame views", () => {
    expect(
      parsePluginView({
        type: "plugin_frame",
        plugin_api: "asset-hub.plugin-api@2",
        title: "Reader",
        url: "/plugins/example.reader/index.html",
      }),
    ).toEqual({
      type: "plugin_frame",
      plugin_api: "asset-hub.plugin-api@2",
      title: "Reader",
      url: "/plugins/example.reader/index.html",
    });

    expect(() =>
      parsePluginView({
        type: "plugin_frame",
        url: "/plugins/example.reader/index.html",
      }),
    ).toThrow();
  });

  it("accepts download views and rejects unknown view kinds", () => {
    expect(
      parsePluginView({
        type: "download",
        url: "/resources/resource-1/download",
        filename: "asset.bin",
      }).type,
    ).toBe("download");
    expect(() =>
      parsePluginView({
        type: "binary_url",
        url: "/resources/resource-1/content",
      }),
    ).toThrow();
  });

  it("rejects untrusted output that does not match a host renderer contract", () => {
    expect(() => parsePluginView({ type: "plugin_frame", url: 42 })).toThrow();
    expect(() => parsePluginView({ type: "script", code: "alert(1)" })).toThrow();
    expect(() => parsePluginView({ type: "table", columns: [], rows: [] })).toThrow();
    expect(() => parsePluginView({ type: "form", schema: {} })).toThrow();
    expect(() => parsePluginView({ type: "text", text: "ok", future: true })).toThrow();
  });
});

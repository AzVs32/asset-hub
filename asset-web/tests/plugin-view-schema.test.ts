import { describe, expect, it } from "vitest";
import { parsePluginView } from "@/infrastructure/http/plugin-view-schema";

describe("plugin view boundary", () => {
  it("accepts a declared view contract", () => {
    expect(
      parsePluginView({
        view: "table",
        columns: [{ key: "name", label: "Name" }],
        rows: [{ name: "Asset" }],
      }).view,
    ).toBe("table");
  });

  it("rejects untrusted output that does not match a host renderer contract", () => {
    expect(() => parsePluginView({ view: "plugin_frame", url: 42 })).toThrow();
    expect(() => parsePluginView({ view: "script", code: "alert(1)" })).toThrow();
  });
});

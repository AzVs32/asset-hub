import { describe, expect, it, vi } from "vitest";
import { pluginFrameUrl } from "@/plugins/frame-boundary";

describe("Plugin Frame asset boundary", () => {
  it("resolves only canonical plugin asset paths", () => {
    const resolveUrl = vi.fn((path: string) => `http://asset-hub.local${path}`);

    expect(pluginFrameUrl("/plugins/example.plugin/index.html", resolveUrl)).toBe(
      "http://asset-hub.local/plugins/example.plugin/index.html",
    );
    for (const path of [
      "https://example.com/index.html",
      "/plugins/Example/index.html",
      "/plugins/example.plugin/../private.html",
      "/plugins/example.plugin/%2e%2e/private.html",
      "/plugins/example.plugin/%2Fprivate.html",
    ]) {
      expect(pluginFrameUrl(path, resolveUrl)).toBeNull();
    }
    expect(resolveUrl).toHaveBeenCalledTimes(1);
  });
});

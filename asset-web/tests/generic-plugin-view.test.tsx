import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { AssetGateway } from "@/application/ports/asset-gateway";
import { GenericPluginViewRenderer } from "@/plugins/renderers/generic-plugin-view";

describe("generic plugin HTML view", () => {
  it("injects a CSP that denies network access", () => {
    const markup = renderToStaticMarkup(
      <GenericPluginViewRenderer
        gateway={{} as AssetGateway}
        view={{ view: "html", html: '<img src="https://example.com/tracker.png">' }}
      />,
    );

    expect(markup).toContain("Content-Security-Policy");
    expect(markup).toContain("default-src");
  });
});

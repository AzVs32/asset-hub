import { describe, expect, it } from "vitest";
import { OpenApiAssetGateway } from "@/infrastructure/http/openapi-asset-gateway";

describe("OpenApiAssetGateway URL boundary", () => {
  const gateway = new OpenApiAssetGateway("/api");

  it("resolves backend-owned asset paths", () => {
    expect(gateway.assetUrl("/plugins/example/view.html")).toBe("/api/plugins/example/view.html");
  });

  it("rejects external and protocol-relative plugin URLs", () => {
    expect(gateway.assetUrl("https://example.com/tracker")).toBeNull();
    expect(gateway.assetUrl("//example.com/tracker")).toBeNull();
    expect(gateway.assetUrl("plugins/example/view.html")).toBeNull();
  });
});

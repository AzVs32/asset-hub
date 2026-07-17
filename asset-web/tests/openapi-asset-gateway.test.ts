import { afterEach, describe, expect, it, vi } from "vitest";
import { OpenApiAssetGateway } from "@/infrastructure/http/openapi-asset-gateway";

afterEach(() => vi.unstubAllGlobals());

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

  it("maps kind metadata schemas and layered resource metadata", async () => {
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(
        jsonResponse({
          items: [
            {
              kind: "core:image",
              parent: "core:file",
              ancestors: ["core:file"],
              label: "Image",
              supports_content: true,
              source: "builtin",
              actions: [],
              detect: null,
              metadata: {
                schema_version: 1,
                schema: {
                  type: "object",
                  readOnly: true,
                  properties: { width: { type: "integer" } },
                },
              },
            },
          ],
        }),
      )
      .mockResolvedValueOnce(jsonResponse(apiResource()));
    vi.stubGlobal("fetch", fetchMock);
    const apiGateway = new OpenApiAssetGateway("http://example.test/api");

    const kinds = await apiGateway.listResourceKinds();
    const resource = await apiGateway.findResource("resource-1");

    expect(kinds[0]?.metadata).toEqual({
      schemaVersion: 1,
      schema: {
        type: "object",
        readOnly: true,
        properties: { width: { type: "integer" } },
      },
    });
    expect(resource.metadata.kindMetadata.layers).toEqual([
      { kind: "core:image", schemaVersion: 1, data: { width: 640, height: 480 } },
    ]);
  });

  it("sends an isolated upsert/clear metadata patch", async () => {
    const fetchMock = vi.fn<typeof fetch>().mockResolvedValueOnce(jsonResponse(apiResource()));
    vi.stubGlobal("fetch", fetchMock);
    const apiGateway = new OpenApiAssetGateway("http://example.test/api");

    await apiGateway.patchKindMetadata("resource-1", {
      upsert: [{ kind: "example:photo", schemaVersion: 2, data: { camera: "A1" } }],
      clear: ["example:legacy"],
    });

    const request = fetchMock.mock.calls[0]?.[0];
    expect(request).toBeInstanceOf(Request);
    await expect((request as Request).clone().json()).resolves.toEqual({
      metadata: {
        kind_metadata: {
          upsert: [
            {
              kind: "example:photo",
              schema_version: 2,
              data: { camera: "A1" },
            },
          ],
          clear: ["example:legacy"],
        },
      },
    });
  });
});

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function apiResource() {
  return {
    id: "resource-1",
    name: "photo.png",
    directory: "images",
    kind: "core:image",
    status: "active",
    metadata: {
      summary: { description: null, tags: [] },
      kind_metadata: {
        layers: [
          {
            kind: "core:image",
            schema_version: 1,
            data: { width: 640, height: 480 },
          },
        ],
      },
    },
    content: null,
    actions: { available_actions: [] },
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    deleted_at: null,
  };
}

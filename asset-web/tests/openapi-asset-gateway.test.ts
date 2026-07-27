import { afterEach, describe, expect, it, vi } from "vitest";
import { OpenApiAssetGateway } from "@/infrastructure/http/openapi-asset-gateway";
import { action, resource } from "./fixtures";

describe("OpenApiAssetGateway URL boundary", () => {
  const gateway = new OpenApiAssetGateway("/api");

  afterEach(() => vi.unstubAllGlobals());

  it("resolves backend-owned asset paths", () => {
    expect(gateway.assetUrl("/plugins/example/view.html")).toBe("/api/plugins/example/view.html");
  });

  it("rejects external and protocol-relative plugin URLs", () => {
    expect(gateway.assetUrl("https://example.com/tracker")).toBeNull();
    expect(gateway.assetUrl("//example.com/tracker")).toBeNull();
    expect(gateway.assetUrl("plugins/example/view.html")).toBeNull();
  });

  it("executes handlerless builtin actions through the action endpoint", async () => {
    const fetchMock = vi.fn(async (_request: Request) =>
      Response.json({
        resource_id: "resource-1",
        action: "legacy.content",
        diagnostics: [],
        view: { view: "json", data: { executed: true } },
      }),
    );
    vi.stubGlobal("fetch", fetchMock);
    const absoluteGateway = new OpenApiAssetGateway("http://localhost/api");
    const item = resource([
      action({
        id: "legacy.content",
        executor: { type: "builtin", handler: null },
        output: { views: [] },
      }),
    ]);

    await absoluteGateway.executeAction(item, "legacy.content");

    const request = fetchMock.mock.calls[0]?.[0];
    if (!request) throw new Error("request was not captured");
    expect(new URL(request.url).pathname).toBe("/api/resources/resource-1/actions/legacy.content");
    expect(request.method).toBe("POST");
  });

  it("creates users without submitting a workspace directory", async () => {
    const fetchMock = vi.fn(async (_request: Request) =>
      Response.json(
        {
          user: {
            id: "user-1",
            username: "alice",
            is_admin: false,
          },
        },
        { status: 201 },
      ),
    );
    vi.stubGlobal("fetch", fetchMock);
    const absoluteGateway = new OpenApiAssetGateway("http://localhost/api");

    await absoluteGateway.createUser({
      username: "alice",
      password: "alice-password",
      isAdmin: false,
    });

    const request = fetchMock.mock.calls[0]?.[0];
    if (!request) throw new Error("request was not captured");
    const body = JSON.parse(await request.text());
    expect(body).toEqual({
      username: "alice",
      password: "alice-password",
      is_admin: false,
    });
    expect(body).not.toHaveProperty("workspace_directory");
  });

  it("preserves spaces in upload names and directories", async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL) =>
      Response.json(
        {
          id: "resource-1",
          name: " draft  01.txt ",
          directory: " library /project A ",
          kind: "core:resource",
          tags: [],
          content: null,
          actions: { available_actions: [] },
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
        },
        { status: 201 },
      ),
    );
    vi.stubGlobal("fetch", fetchMock);

    const resource = await gateway.uploadResource({
      file: new File(["content"], "fallback.txt", { type: "text/plain" }),
      name: " draft  01.txt ",
      directory: " library /project A ",
      kind: "core:resource",
      tags: "",
    });

    const requestUrl = new URL(String(fetchMock.mock.calls[0]?.[0]), "http://localhost");
    expect(requestUrl.searchParams.get("name")).toBe(" draft  01.txt ");
    expect(requestUrl.searchParams.get("directory")).toBe(" library /project A ");
    expect(resource.name).toBe(" draft  01.txt ");
    expect(resource.directory).toBe(" library /project A ");
  });
});

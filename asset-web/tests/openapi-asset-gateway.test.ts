import { afterEach, describe, expect, it, vi } from "vitest";
import type { BlobSha256, FileSha256 } from "@/infrastructure/http/file-sha256";
import { OpenApiAssetGateway } from "@/infrastructure/http/openapi-asset-gateway";
import { action, resource } from "./fixtures";

describe("OpenApiAssetGateway URL boundary", () => {
  const hashFile: FileSha256 = async (file, onProgress) => {
    onProgress?.(file.size);
    return "ed7002b439e9ac845f22357d822bac14447368f3032e885031b31f6f2f88a3f8";
  };
  const hashChunk: BlobSha256 = async () => "c".repeat(64);
  const gateway = new OpenApiAssetGateway("/api", hashFile, hashChunk);

  afterEach(() => {
    vi.unstubAllGlobals();
    localStorage.clear();
  });

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
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json({ id: "upload-1", offset: 0, size: 7, status: "uploading" }, { status: 201 }),
      )
      .mockResolvedValueOnce(new Response(null, { status: 204, headers: { "Upload-Offset": "7" } }))
      .mockResolvedValueOnce(
        Response.json(
          {
            id: "upload-1",
            offset: 7,
            size: 7,
            status: "finalizing",
          },
          { status: 202 },
        ),
      );
    vi.stubGlobal("fetch", fetchMock);

    const progress: Array<{ stage: string; bytesSent: number; totalBytes: number }> = [];
    const receipt = await gateway.uploadResource(
      {
        file: new File(["content"], "fallback.txt", { type: "text/plain" }),
        name: " draft  01.txt ",
        directory: " library /project A ",
        kind: "core:resource",
        tags: "",
      },
      (event) => progress.push(event),
    );

    const createUrl = new URL(String(fetchMock.mock.calls[0]?.[0]), "http://localhost");
    expect(createUrl.pathname).toBe("/api/uploads");
    const createBody = JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body));
    expect(createBody.name).toBe(" draft  01.txt ");
    expect(createBody.directory).toBe(" library /project A ");
    expect(createBody.expected_sha256).toBe(
      "ed7002b439e9ac845f22357d822bac14447368f3032e885031b31f6f2f88a3f8",
    );
    expect(fetchMock.mock.calls[1]?.[1]?.headers).toMatchObject({
      "Upload-Offset": "0",
      "Upload-Checksum": "c".repeat(64),
    });
    expect(String(fetchMock.mock.calls[2]?.[0])).toContain("/uploads/upload-1/complete");
    expect(receipt).toEqual({ id: "upload-1", name: " draft  01.txt " });
    expect(progress).toEqual([
      { stage: "preparing", bytesSent: 0, totalBytes: 7 },
      { stage: "preparing", bytesSent: 7, totalBytes: 7 },
      { stage: "uploading", bytesSent: 0, totalBytes: 7 },
      { stage: "uploading", bytesSent: 7, totalBytes: 7 },
      { stage: "finalizing", bytesSent: 7, totalBytes: 7 },
    ]);
  });

  it("retransmits a chunk when the server rejects its checksum", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json({ id: "upload-1", offset: 0, size: 7, status: "uploading" }, { status: 201 }),
      )
      .mockResolvedValueOnce(
        Response.json(
          { error: "upload chunk checksum mismatch: expected c, actual d" },
          { status: 409 },
        ),
      )
      .mockResolvedValueOnce(new Response(null, { status: 204, headers: { "Upload-Offset": "7" } }))
      .mockResolvedValueOnce(
        Response.json(
          { id: "upload-1", offset: 7, size: 7, status: "finalizing" },
          { status: 202 },
        ),
      );
    vi.stubGlobal("fetch", fetchMock);

    await gateway.uploadResource({
      file: new File(["content"], "retry.txt", { type: "text/plain" }),
      name: "retry.txt",
      directory: "uploads",
      kind: "core:resource",
      tags: "",
    });

    const patchRequests = fetchMock.mock.calls.filter(([, init]) => init?.method === "PATCH");
    expect(patchRequests).toHaveLength(2);
    expect(patchRequests[0]?.[1]?.headers).toEqual(patchRequests[1]?.[1]?.headers);
    expect(patchRequests[0]?.[1]?.body).toBe(patchRequests[1]?.[1]?.body);
  });

  it("continues a persisted upload from the server offset", async () => {
    const uploadGateway = new OpenApiAssetGateway("http://localhost/api", hashFile, hashChunk);
    const file = new File(["content"], "resume.txt", {
      type: "text/plain",
      lastModified: 1_700_000_000_000,
    });
    const draft = {
      file,
      name: "resume.txt",
      directory: "uploads",
      kind: "core:resource",
      tags: "",
    };
    const failedFetch = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json(
          { id: "upload-resume", offset: 0, size: 7, status: "uploading" },
          { status: 201 },
        ),
      )
      .mockResolvedValueOnce(Response.json({ error: "connection lost" }, { status: 500 }));
    vi.stubGlobal("fetch", failedFetch);

    await expect(uploadGateway.uploadResource(draft)).rejects.toThrow("connection lost");

    const resumedFetch = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json({ id: "upload-resume", offset: 4, size: 7, status: "uploading" }),
      )
      .mockResolvedValueOnce(new Response(null, { status: 204, headers: { "Upload-Offset": "7" } }))
      .mockResolvedValueOnce(
        Response.json(
          { id: "upload-resume", offset: 7, size: 7, status: "finalizing" },
          { status: 202 },
        ),
      );
    vi.stubGlobal("fetch", resumedFetch);

    const receipt = await uploadGateway.uploadResource(draft);

    expect(resumedFetch.mock.calls[0]?.[1]?.method).toBeUndefined();
    const resumedPatch = resumedFetch.mock.calls[1]?.[1];
    expect(resumedPatch?.headers).toMatchObject({
      "Upload-Offset": "4",
      "Upload-Checksum": "c".repeat(64),
    });
    if (!(resumedPatch?.body instanceof Blob)) throw new Error("resumed chunk was not captured");
    expect(await resumedPatch.body.text()).toBe("ent");
    expect(receipt.id).toBe("upload-resume");

    const completionFetch = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json({
          id: "upload-resume",
          offset: 7,
          size: 7,
          status: "completed",
          resource_id: "resource-resumed",
        }),
      )
      .mockResolvedValueOnce(
        Response.json(
          {
            id: "resource-resumed",
            name: "resume.txt",
            directory: "uploads",
            kind: "core:resource",
            tags: [],
            content: null,
            actions: { available_actions: [] },
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-01T00:00:00Z",
          },
          { status: 201 },
        ),
      )
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", completionFetch);
    const completionGateway = new OpenApiAssetGateway("http://localhost/api");

    const uploaded = await completionGateway.waitForUpload(receipt.id);

    expect(uploaded.id).toBe("resource-resumed");
  });

  it("does not resume a same-name same-size file with a different SHA-256", async () => {
    const file = new File(["content"], "same.txt", {
      type: "text/plain",
      lastModified: 1_700_000_000_000,
    });
    const draft = {
      file,
      name: "same.txt",
      directory: "uploads",
      kind: "core:resource",
      tags: "",
    };
    const firstFetch = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json(
          { id: "upload-old", offset: 0, size: 7, status: "uploading" },
          { status: 201 },
        ),
      )
      .mockResolvedValueOnce(Response.json({ error: "connection lost" }, { status: 500 }));
    vi.stubGlobal("fetch", firstFetch);
    await expect(
      new OpenApiAssetGateway("http://localhost/api", hashFile, hashChunk).uploadResource(draft),
    ).rejects.toThrow("connection lost");

    const differentHash: FileSha256 = async () => "b".repeat(64);
    const secondFetch = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json(
          { id: "upload-new", offset: 0, size: 7, status: "uploading" },
          { status: 201 },
        ),
      )
      .mockResolvedValueOnce(new Response(null, { status: 204, headers: { "Upload-Offset": "7" } }))
      .mockResolvedValueOnce(
        Response.json(
          { id: "upload-new", offset: 7, size: 7, status: "finalizing" },
          { status: 202 },
        ),
      );
    vi.stubGlobal("fetch", secondFetch);

    const receipt = await new OpenApiAssetGateway(
      "http://localhost/api",
      differentHash,
      hashChunk,
    ).uploadResource(draft);

    expect(new URL(String(secondFetch.mock.calls[0]?.[0])).pathname).toBe("/api/uploads");
    expect(secondFetch.mock.calls[0]?.[1]?.method).toBe("POST");
    expect(receipt.id).toBe("upload-new");
  });
});

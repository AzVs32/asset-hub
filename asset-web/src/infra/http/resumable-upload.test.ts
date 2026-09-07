// @vitest-environment node
import { afterEach, expect, it, vi } from "vitest";
import { ResumableUpload } from "./resumable-upload";

afterEach(() => vi.unstubAllGlobals());

it("recovers publication tracking after a lost completion response and keeps full-file hashes in resume identity", async () => {
  const storage = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  });
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  const session = { id: "session", offset: 0, size: 1, status: "uploading" };
  const receipt = () => Response.json(session);
  fetch
    .mockResolvedValueOnce(receipt())
    .mockResolvedValueOnce(new Response(null, { headers: { "Upload-Offset": "1" } }))
    .mockRejectedValueOnce(new TypeError("Completion response lost"));
  const findResource = vi.fn();
  const create = (hash: string) =>
    new ResumableUpload(
      "/api",
      async () => hash,
      async () => "chunk-hash",
      findResource,
      async () => "directory",
    );
  const file = new File(["a"], "same.txt", { lastModified: 1 });
  await expect(create("hash-a").upload({ file, directory: "", name: "asset" })).rejects.toThrow(
    "Completion response lost",
  );
  const restored = create("hash-a");
  expect(restored.pendingUploads()).toEqual([{ id: "session", name: "asset" }]);
  const fingerprint = Object.keys(JSON.parse(Array.from(storage.values())[0] ?? "{}"))[0];
  expect(JSON.parse(fingerprint ?? "{}").file.sha256).toBe("hash-a");

  // A failed status request must not delete the resumable session or claim server failure.
  fetch.mockRejectedValueOnce(new TypeError("offline"));
  await expect(restored.status("session")).rejects.toThrow("offline");
  expect(restored.pendingUploads()).toHaveLength(1);
  fetch.mockResolvedValueOnce(Response.json({ ...session, offset: 1, status: "finalizing" }));
  expect(await restored.status("session")).toEqual({ status: "finalizing" });

  // Same metadata and size with different content must start a fresh session.
  fetch.mockClear();
  fetch
    .mockResolvedValueOnce(Response.json({ ...session, id: "new-session" }))
    .mockResolvedValueOnce(new Response(null, { headers: { "Upload-Offset": "1" } }))
    .mockResolvedValueOnce(
      Response.json({ ...session, id: "new-session", offset: 1, status: "finalizing" }),
    );
  await create("hash-b").upload({
    file: new File(["b"], "same.txt", { lastModified: 1 }),
    directory: "",
    name: "asset",
  });
  expect(fetch.mock.calls[0]).toMatchObject(["/api/uploads", { method: "POST" }]);
});

it("passes cancellation through the status request", async () => {
  const fetch = vi.fn(
    (_url: string, { signal }: { signal: AbortSignal }) =>
      new Promise<Response>((_resolve, reject) => {
        signal.addEventListener("abort", () => reject(signal.reason), { once: true });
      }),
  );
  vi.stubGlobal("fetch", fetch);
  const upload = new ResumableUpload("/api", vi.fn(), vi.fn(), vi.fn(), vi.fn());
  const controller = new AbortController();
  const status = upload.status("session", controller.signal);
  controller.abort();
  await expect(status).rejects.toMatchObject({ name: "AbortError" });
});

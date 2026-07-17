import { describe, expect, it } from "vitest";
import { parseApiResponse } from "../src/apiResponse";

describe("parseApiResponse", () => {
  it("parses successful JSON responses", async () => {
    await expect(parseApiResponse(new Response('{"ok":true}'))).resolves.toEqual({ ok: true });
  });

  it("uses the API error message", async () => {
    await expect(parseApiResponse(new Response('{"error":"denied"}', { status: 403 })))
      .rejects.toThrow("denied");
  });

  it("rejects non-JSON API responses explicitly", async () => {
    await expect(parseApiResponse(new Response("proxy failure", { status: 502 })))
      .rejects.toThrow("invalid JSON");
  });
});


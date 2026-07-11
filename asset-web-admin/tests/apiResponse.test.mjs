import assert from "node:assert/strict";
import test from "node:test";
import { parseApiResponse } from "../src/apiResponse.js";

test("parses successful JSON responses", async () => {
  assert.deepEqual(await parseApiResponse(new Response('{"ok":true}')), { ok: true });
});

test("uses the API error message", async () => {
  await assert.rejects(
    () => parseApiResponse(new Response('{"error":"denied"}', { status: 403 })),
    /denied/,
  );
});

test("rejects non-JSON API responses explicitly", async () => {
  await assert.rejects(
    () => parseApiResponse(new Response("proxy failure", { status: 502 })),
    /invalid JSON/,
  );
});

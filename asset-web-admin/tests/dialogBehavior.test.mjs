import assert from "node:assert/strict";
import test from "node:test";
import { shouldCloseAfterCreate } from "../src/features/resourceWorkspace/dialogBehavior.js";

test("folder dialog stays open when creation fails", () => {
  assert.equal(shouldCloseAfterCreate(undefined), false);
  assert.equal(shouldCloseAfterCreate(null), false);
});

test("folder dialog closes after successful creation", () => {
  assert.equal(shouldCloseAfterCreate({ path: "docs" }), true);
});

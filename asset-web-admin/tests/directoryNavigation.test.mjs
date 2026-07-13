import assert from "node:assert/strict";
import test from "node:test";
import {
  directoryBreadcrumbs,
  parentDirectoryWithinRoot,
} from "../src/features/resourceWorkspace/directoryNavigation.js";

test("member home is the visible navigation root", () => {
  assert.deepEqual(directoryBreadcrumbs("teams/alice", "teams/alice", "Home"), [
    { label: "Home", path: "teams/alice" },
  ]);
  assert.equal(parentDirectoryWithinRoot("teams/alice", "teams/alice"), null);
});

test("member can navigate inside home but not above it", () => {
  assert.deepEqual(directoryBreadcrumbs("teams/alice/photos/raw", "teams/alice", "Home"), [
    { label: "Home", path: "teams/alice" },
    { label: "photos", path: "teams/alice/photos" },
    { label: "raw", path: "teams/alice/photos/raw" },
  ]);
  assert.equal(
    parentDirectoryWithinRoot("teams/alice/photos", "teams/alice"),
    "teams/alice",
  );
});

test("administrator navigation still reaches the system root", () => {
  assert.deepEqual(directoryBreadcrumbs("teams/alice", "", "Root"), [
    { label: "Root", path: "" },
    { label: "teams", path: "teams" },
    { label: "alice", path: "teams/alice" },
  ]);
  assert.equal(parentDirectoryWithinRoot("teams", ""), "");
  assert.equal(parentDirectoryWithinRoot("", ""), null);
});

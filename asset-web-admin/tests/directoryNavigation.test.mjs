import assert from "node:assert/strict";
import test from "node:test";
import {
  directoryBreadcrumbs,
  parentDirectoryWithinRoot,
} from "../src/features/resourceWorkspace/directoryNavigation.js";

test("member workspace is the visible navigation root", () => {
  assert.deepEqual(directoryBreadcrumbs("teams/alice", "teams/alice", "Workspace"), [
    { label: "Workspace", path: "teams/alice" },
  ]);
  assert.equal(parentDirectoryWithinRoot("teams/alice", "teams/alice"), null);
});

test("member can navigate inside workspace but not above it", () => {
  assert.deepEqual(directoryBreadcrumbs("teams/alice/photos/raw", "teams/alice", "Workspace"), [
    { label: "Workspace", path: "teams/alice" },
    { label: "photos", path: "teams/alice/photos" },
    { label: "raw", path: "teams/alice/photos/raw" },
  ]);
  assert.equal(
    parentDirectoryWithinRoot("teams/alice/photos", "teams/alice"),
    "teams/alice",
  );
});

test("additional grant can act as an independent navigation root", () => {
  assert.deepEqual(directoryBreadcrumbs("shared/photos/raw", "shared/photos", "shared/photos"), [
    { label: "shared/photos", path: "shared/photos" },
    { label: "raw", path: "shared/photos/raw" },
  ]);
  assert.equal(parentDirectoryWithinRoot("shared/photos", "shared/photos"), null);
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

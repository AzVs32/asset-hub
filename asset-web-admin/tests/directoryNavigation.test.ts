import { describe, expect, it } from "vitest";
import {
  directoryBreadcrumbs,
  parentDirectoryWithinRoot,
} from "../src/features/resourceWorkspace/directoryNavigation";

describe("directory navigation", () => {
  it("uses the member workspace as the visible root", () => {
    expect(directoryBreadcrumbs("teams/alice", "teams/alice", "Workspace")).toEqual([
      { label: "Workspace", path: "teams/alice" },
    ]);
    expect(parentDirectoryWithinRoot("teams/alice", "teams/alice")).toBeNull();
  });

  it("keeps member navigation inside its active root", () => {
    expect(directoryBreadcrumbs("teams/alice/photos/raw", "teams/alice", "Workspace")).toEqual([
      { label: "Workspace", path: "teams/alice" },
      { label: "photos", path: "teams/alice/photos" },
      { label: "raw", path: "teams/alice/photos/raw" },
    ]);
    expect(parentDirectoryWithinRoot("teams/alice/photos", "teams/alice"))
      .toBe("teams/alice");
  });

  it("supports independent grant roots and the administrator root", () => {
    expect(directoryBreadcrumbs("shared/photos/raw", "shared/photos", "shared/photos")).toEqual([
      { label: "shared/photos", path: "shared/photos" },
      { label: "raw", path: "shared/photos/raw" },
    ]);
    expect(parentDirectoryWithinRoot("shared/photos", "shared/photos")).toBeNull();
    expect(parentDirectoryWithinRoot("teams", "")).toBe("");
    expect(parentDirectoryWithinRoot("", "")).toBeNull();
  });
});


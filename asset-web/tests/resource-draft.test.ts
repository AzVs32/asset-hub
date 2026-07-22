import { describe, expect, it } from "vitest";
import { breadcrumbs, normalizeDirectory, parentDirectory } from "@/domain/resource-draft";

describe("resource directory navigation", () => {
  it("normalizes separators and empty segments without changing spaces", () => {
    expect(normalizeDirectory("/ library /project  A/./drafts /")).toBe(
      " library /project  A/drafts ",
    );
  });

  it("does not navigate above a member access root", () => {
    expect(parentDirectory("members/alice/project", "members/alice")).toBe("members/alice");
    expect(parentDirectory("members/alice", "members/alice")).toBeNull();
  });

  it("builds breadcrumbs relative to the access root", () => {
    expect(breadcrumbs("members/alice/project/raw", "members/alice")).toEqual([
      { path: "members/alice", label: "alice" },
      { path: "members/alice/project", label: "project" },
      { path: "members/alice/project/raw", label: "raw" },
    ]);
  });
});
